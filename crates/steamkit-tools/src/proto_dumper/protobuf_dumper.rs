//! Turns protobuf descriptors back into `.proto` source.
//!
//! The descriptor types come from `prost_types`.

use std::collections::{BTreeSet, HashMap, HashSet};

use prost_types::field_descriptor_proto::{Label, Type};
use prost_types::{
    DescriptorProto, EnumDescriptorProto, EnumOptions, EnumValueOptions, ExtensionRangeOptions,
    FieldDescriptorProto, FieldOptions, FileDescriptorProto, FileOptions, MessageOptions,
    MethodOptions, OneofOptions, ServiceDescriptorProto, ServiceOptions,
};

use crate::proto_dumper::raw_options::{
    RawDescriptor, RawEnumDescriptor, RawFieldDescriptor, RawFileDescriptor, RawMessage,
    RawMessageDescriptor, RawServiceDescriptor,
};
use crate::proto_dumper::util::{
    to_literal, to_literal_bytes, to_literal_double, to_literal_float, OptionList,
};

/// The largest field number a protobuf message can use.
const MAX_FIELD_NUMBER: i32 = 536_870_911;

/// The name of the `FileOptions` message in the protobuf descriptor schema.
const FILE_OPTIONS_TYPE: &str = ".google.protobuf.FileOptions";

/// The name of the `FieldOptions` message in the protobuf descriptor schema.
const FIELD_OPTIONS_TYPE: &str = ".google.protobuf.FieldOptions";

/// The name of the `MessageOptions` message in the protobuf descriptor schema.
const MESSAGE_OPTIONS_TYPE: &str = ".google.protobuf.MessageOptions";

/// The name of the `EnumOptions` message in the protobuf descriptor schema.
const ENUM_OPTIONS_TYPE: &str = ".google.protobuf.EnumOptions";

/// The name of the `EnumValueOptions` message in the protobuf descriptor schema.
const ENUM_VALUE_OPTIONS_TYPE: &str = ".google.protobuf.EnumValueOptions";

/// The name of the `ServiceOptions` message in the protobuf descriptor schema.
const SERVICE_OPTIONS_TYPE: &str = ".google.protobuf.ServiceOptions";

/// The name of the `MethodOptions` message in the protobuf descriptor schema.
const METHOD_OPTIONS_TYPE: &str = ".google.protobuf.MethodOptions";

/// The name of the `OneofOptions` message in the protobuf descriptor schema.
const ONEOF_OPTIONS_TYPE: &str = ".google.protobuf.OneofOptions";

/// The name of the `ExtensionRangeOptions` message in the protobuf descriptor schema.
const EXTENSION_RANGE_OPTIONS_TYPE: &str = ".google.protobuf.ExtensionRangeOptions";

/// A reference to another file.
#[derive(Clone, Copy, Debug)]
enum DependencyRef {
    /// A file that was collected.
    Known(usize),
    /// A file that was referenced but never found.
    Missing(usize),
}

/// A file that was collected.
#[derive(Clone, Debug)]
struct ProtoNode {
    /// The name of the file.
    name: String,
    /// The descriptor of the file; missing for files that were not found.
    proto: Option<FileDescriptorProto>,
    /// The files this file depends on.
    dependencies: Vec<DependencyRef>,
    /// The files whose types this file can refer to.
    all_public_dependencies: HashSet<usize>,
    /// The types this file declares or refers to.
    types: Vec<usize>,
    /// False when the file was referenced but never found.
    defined: bool,
}

/// The kind of descriptor a type was declared by.
#[derive(Clone, Debug)]
enum TypeSource {
    /// A protobuf extension.
    Extension(FieldDescriptorProto),
    /// An enum.
    Enum(EnumDescriptorProto),
    /// A message.
    Message(DescriptorProto),
    /// A service; the descriptor is kept for the sake of the type map.
    #[allow(dead_code)]
    Service(ServiceDescriptorProto),
}

/// A type that was declared by, or referred to by, a descriptor.
#[derive(Clone, Debug)]
struct ProtoTypeNode {
    /// The fully qualified name of the type.
    name: String,
    /// The file the type was declared in.
    proto: Option<usize>,
    /// The descriptor of the type.
    source: Option<TypeSource>,
    /// False when the type was referred to but never declared.
    defined: bool,
}

/// Dumps protobuf descriptors as `.proto` source.
pub struct ProtobufDumper {
    /// The collected descriptors.
    descriptors: Vec<RawDescriptor>,
    /// The raw view of every collected descriptor.
    raw_files: Vec<RawFileDescriptor>,
    /// The collected files, in the order they were found.
    protobuf_map: Vec<ProtoNode>,
    /// The index of every collected file, by name.
    protobuf_map_index: HashMap<String, usize>,
    /// The files that were referenced but never found.
    missing_files: Vec<ProtoNode>,
    /// Every type that was declared by, or referred to by, a descriptor.
    protobuf_type_map: Vec<ProtoTypeNode>,
    /// The index of every type, by name.
    protobuf_type_map_index: HashMap<String, usize>,
    /// The files that hold the options that were used by the file being dumped.
    option_imports: BTreeSet<String>,
}

impl ProtobufDumper {
    /// Creates a dumper for the given descriptors.
    pub fn new(descriptors: Vec<RawDescriptor>) -> Self {
        let raw_files = descriptors.iter().map(RawDescriptor::raw).collect();

        ProtobufDumper {
            descriptors,
            raw_files,
            protobuf_map: Vec::new(),
            protobuf_map_index: HashMap::new(),
            missing_files: Vec::new(),
            protobuf_type_map: Vec::new(),
            protobuf_type_map_index: HashMap::new(),
            option_imports: BTreeSet::new(),
        }
    }

    /// Checks that every dependency and every type could be resolved.
    ///
    /// Returns false when something is missing; the problems are printed to
    /// the console.
    pub fn analyze(&mut self) -> bool {
        for index in 0..self.descriptors.len() {
            let proto = self.descriptors[index].proto.clone();
            let name = proto.name.clone().unwrap_or_default();
            let package = proto.package.clone().unwrap_or_default();

            self.protobuf_map.push(ProtoNode {
                name: name.clone(),
                proto: Some(proto.clone()),
                dependencies: Vec::new(),
                all_public_dependencies: HashSet::new(),
                types: Vec::new(),
                defined: true,
            });
            self.protobuf_map_index.insert(name, index);

            for extension in &proto.extension {
                self.record_type(
                    index,
                    get_package_path(&package, extension.name.as_deref().unwrap_or("")),
                    Some(index),
                    Some(TypeSource::Extension(extension.clone())),
                );

                if is_named_type(extension.r#type) && !extension.type_name.as_deref().unwrap_or("").is_empty()
                {
                    self.record_type(
                        index,
                        get_package_path(&package, extension.type_name.as_deref().unwrap_or("")),
                        None,
                        None,
                    );
                }

                if !extension.extendee.as_deref().unwrap_or("").is_empty() {
                    self.record_type(
                        index,
                        get_package_path(&package, extension.extendee.as_deref().unwrap_or("")),
                        None,
                        None,
                    );
                }
            }

            for enum_type in &proto.enum_type {
                self.record_type(
                    index,
                    get_package_path(&package, enum_type.name.as_deref().unwrap_or("")),
                    Some(index),
                    Some(TypeSource::Enum(enum_type.clone())),
                );
            }

            for message in &proto.message_type {
                self.analyze_message(message, index, &package);
            }

            for service in &proto.service {
                self.record_type(
                    index,
                    get_package_path(&package, service.name.as_deref().unwrap_or("")),
                    Some(index),
                    Some(TypeSource::Service(service.clone())),
                );

                for method in &service.method {
                    let input_type = method.input_type.as_deref().unwrap_or("");
                    if !input_type.is_empty() {
                        self.record_type(index, get_package_path(&package, input_type), None, None);
                    }

                    let output_type = method.output_type.as_deref().unwrap_or("");
                    if !output_type.is_empty() {
                        self.record_type(index, get_package_path(&package, output_type), None, None);
                    }
                }
            }
        }

        self.collect_dependencies();

        for index in 0..self.protobuf_map.len() {
            let undefined_files: Vec<String> = self.protobuf_map[index]
                .dependencies
                .iter()
                .filter_map(|dependency| match dependency {
                    DependencyRef::Known(index) => {
                        let node = &self.protobuf_map[*index];
                        if node.defined {
                            None
                        } else {
                            Some(node.name.clone())
                        }
                    }
                    DependencyRef::Missing(index) => Some(self.missing_files[*index].name.clone()),
                })
                .collect();

            if !undefined_files.is_empty() {
                println!(
                    "Not all dependencies were found for {}",
                    self.protobuf_map[index].name
                );

                for file in undefined_files {
                    println!("Dependency not found: {file}");
                }

                return false;
            }

            let undefined_types: Vec<String> = self.protobuf_map[index]
                .types
                .iter()
                .filter(|index| !self.protobuf_type_map[**index].defined)
                .map(|index| self.protobuf_type_map[*index].name.clone())
                .collect();

            if !undefined_types.is_empty() {
                println!(
                    "Not all types were resolved for {}",
                    self.protobuf_map[index].name
                );

                for name in undefined_types {
                    println!("Type not found: {name}");
                }

                return false;
            }

            self.add_public_dependencies(index, 0);
        }

        true
    }

    /// Writes every collected descriptor as `.proto` source.
    pub fn dump_files<F>(&mut self, mut callback: F)
    where
        F: FnMut(&str, &str),
    {
        for index in 0..self.descriptors.len() {
            let mut buffer = String::new();
            self.dump_file_descriptor(index, &mut buffer);

            let name = self.descriptors[index].proto.name.clone().unwrap_or_default();
            callback(&name, &buffer);
        }
    }

    /// Adds a type to the type map, or updates it when it is already known.
    fn add_type(&mut self, name: String, owner: Option<usize>, source: Option<TypeSource>) -> usize {
        if let Some(&index) = self.protobuf_type_map_index.get(&name) {
            if source.is_some() {
                let node = &mut self.protobuf_type_map[index];
                node.proto = owner;
                node.source = source;
                node.defined = true;
            }

            return index;
        }

        let index = self.protobuf_type_map.len();
        let defined = source.is_some();

        self.protobuf_type_map.push(ProtoTypeNode {
            name: name.clone(),
            proto: owner,
            source,
            defined,
        });
        self.protobuf_type_map_index.insert(name, index);

        index
    }

    /// Adds a type to the type map and records it on the file that uses it.
    fn record_type(
        &mut self,
        file_index: usize,
        name: String,
        owner: Option<usize>,
        source: Option<TypeSource>,
    ) {
        let index = self.add_type(name, owner, source);
        self.protobuf_map[file_index].types.push(index);
    }

    /// Walks a message and everything it declares.
    fn analyze_message(&mut self, message: &DescriptorProto, file_index: usize, package_path: &str) {
        let name = message.name.as_deref().unwrap_or("");

        self.record_type(
            file_index,
            get_package_path(package_path, name),
            Some(file_index),
            Some(TypeSource::Message(message.clone())),
        );

        let nested_path = get_package_path(package_path, name);

        for extension in &message.extension {
            self.record_type(
                file_index,
                get_package_path(&nested_path, extension.name.as_deref().unwrap_or("")),
                Some(file_index),
                Some(TypeSource::Extension(extension.clone())),
            );

            if is_named_type(extension.r#type)
                && !extension.type_name.as_deref().unwrap_or("").is_empty()
            {
                self.record_type(
                    file_index,
                    get_package_path(package_path, extension.type_name.as_deref().unwrap_or("")),
                    None,
                    None,
                );
            }

            if !extension.extendee.as_deref().unwrap_or("").is_empty() {
                self.record_type(
                    file_index,
                    get_package_path(package_path, extension.extendee.as_deref().unwrap_or("")),
                    None,
                    None,
                );
            }
        }

        for enum_type in &message.enum_type {
            self.record_type(
                file_index,
                get_package_path(&nested_path, enum_type.name.as_deref().unwrap_or("")),
                Some(file_index),
                Some(TypeSource::Enum(enum_type.clone())),
            );
        }

        for field in &message.field {
            if is_named_type(field.r#type) && !field.type_name.as_deref().unwrap_or("").is_empty() {
                self.record_type(
                    file_index,
                    get_package_path(package_path, field.type_name.as_deref().unwrap_or("")),
                    None,
                    None,
                );
            }

            if !field.extendee.as_deref().unwrap_or("").is_empty() {
                self.record_type(
                    file_index,
                    get_package_path(package_path, field.extendee.as_deref().unwrap_or("")),
                    None,
                    None,
                );
            }
        }

        for nested in &message.nested_type {
            self.analyze_message(nested, file_index, &nested_path);
        }
    }

    /// Resolves the imports of every collected file.
    fn collect_dependencies(&mut self) {
        for index in 0..self.protobuf_map.len() {
            let dependencies = self.protobuf_map[index]
                .proto
                .as_ref()
                .map(|proto| proto.dependency.clone())
                .unwrap_or_default();

            for dependency in dependencies {
                if dependency.starts_with("google") {
                    continue;
                }

                if let Some(&known) = self.protobuf_map_index.get(&dependency) {
                    self.protobuf_map[index].dependencies.push(DependencyRef::Known(known));
                    continue;
                }

                println!(
                    "Unknown dependency: {dependency} for {}",
                    self.protobuf_map[index].name
                );

                let missing = match self
                    .missing_files
                    .iter()
                    .position(|node| node.name == dependency)
                {
                    Some(position) => position,
                    None => {
                        self.missing_files.push(ProtoNode {
                            name: dependency.clone(),
                            proto: None,
                            dependencies: Vec::new(),
                            all_public_dependencies: HashSet::new(),
                            types: Vec::new(),
                            defined: false,
                        });
                        self.missing_files.len() - 1
                    }
                };

                self.protobuf_map[index]
                    .dependencies
                    .push(DependencyRef::Missing(missing));
            }
        }

        for node in self.missing_files.clone() {
            let index = self.protobuf_map.len();
            self.protobuf_map_index
                .insert(node.name.clone(), index);
            self.protobuf_map.push(node);
        }
    }

    /// Collects the files whose types the given file may refer to.
    fn add_public_dependencies(&mut self, node_index: usize, depth: usize) {
        let mut visited = HashSet::new();
        self.add_public_dependencies_inner(node_index, depth, &mut visited);
    }

    /// Collects the files whose types the given file may refer to.
    fn add_public_dependencies_inner(
        &mut self,
        node_index: usize,
        depth: usize,
        visited: &mut HashSet<usize>,
    ) {
        if !visited.insert(node_index) {
            return;
        }

        let proto = match self.protobuf_map[node_index].proto.clone() {
            Some(proto) => proto,
            None => return,
        };

        let dependencies: Vec<String> = if depth == 0 {
            proto.dependency.clone()
        } else {
            proto.public_dependency
                .iter()
                .filter_map(|index| proto.dependency.get(*index as usize).cloned())
                .collect()
        };

        for dependency in dependencies {
            let known = match self.protobuf_map_index.get(&dependency) {
                Some(&index) => index,
                None => continue,
            };

            if !self.protobuf_map[known].defined {
                continue;
            }

            self.protobuf_map[node_index]
                .all_public_dependencies
                .insert(known);

            self.add_public_dependencies_inner(known, depth + 1, visited);
        }
    }

    /// Writes a single file descriptor as `.proto` source.
    fn dump_file_descriptor(&mut self, index: usize, buffer: &mut String) {
        let proto = self.descriptors[index].proto.clone();
        let raw = self.raw_files[index].clone();
        let mut marker = false;

        let syntax = proto.syntax.clone().unwrap_or_default();
        if !syntax.is_empty() {
            append_heading_space(buffer, &mut marker);
            buffer.push_str(&format!("syntax = {};\n", to_literal(&syntax)));
            marker = true;
        }

        for (position, dependency) in proto.dependency.iter().enumerate() {
            let modifier = if proto.public_dependency.contains(&(position as i32)) {
                "public "
            } else if proto.weak_dependency.contains(&(position as i32)) {
                "weak "
            } else {
                ""
            };

            buffer.push_str(&format!("import {modifier}\"{dependency}\";\n"));
            marker = true;
        }

        let imports_end = buffer.len();
        let has_heading = marker;
        self.option_imports.clear();

        let package = proto.package.clone().unwrap_or_default();
        if !package.is_empty() {
            append_heading_space(buffer, &mut marker);
            buffer.push_str(&format!("package {package};\n"));
            marker = true;
        }

        let mut options = OptionList::new();
        self.dump_file_options(
            &proto,
            proto.options.as_ref(),
            raw.options().as_ref(),
            &mut options,
        );

        for (key, value) in options.iter() {
            append_heading_space(buffer, &mut marker);
            buffer.push_str(&format!("option {key} = {value};\n"));
        }

        if !options.is_empty() {
            marker = true;
        }

        self.dump_extension_descriptors(
            &proto,
            &proto.extension,
            raw.extensions(),
            buffer,
            0,
            &mut marker,
        );

        let raw_enums = raw.enums();
        for (index, enum_type) in proto.enum_type.iter().enumerate() {
            let raw_enum = raw_enums.get(index).cloned().unwrap_or_default();
            self.dump_enum_descriptor(&proto, enum_type, &raw_enum, buffer, 0, &mut marker);
        }

        let raw_messages = raw.messages();
        for (index, message) in proto.message_type.iter().enumerate() {
            let raw_message = raw_messages.get(index).cloned().unwrap_or_default();
            self.dump_descriptor(&proto, message, &raw_message, buffer, 0, &mut marker);
        }

        let raw_services = raw.services();
        for (index, service) in proto.service.iter().enumerate() {
            let raw_service = raw_services.get(index).cloned().unwrap_or_default();
            self.dump_service(&proto, service, &raw_service, buffer, &mut marker);
        }

        if !self.option_imports.is_empty() {
            let mut imports = String::new();
            for import in &self.option_imports {
                imports.push_str(&format!("import \"{import}\";\n"));
            }

            if !has_heading && imports_end < buffer.len() {
                imports.push('\n');
            }

            buffer.insert_str(imports_end, &imports);
        }
    }

    /// Writes the extensions of a file or a message.
    fn dump_extension_descriptors(
        &mut self,
        source: &FileDescriptorProto,
        fields: &[FieldDescriptorProto],
        raws: Vec<RawFieldDescriptor>,
        buffer: &mut String,
        level: usize,
        marker: &mut bool,
    ) {
        let levelspace = "\t".repeat(level);

        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (index, field) in fields.iter().enumerate() {
            let extendee = field.extendee.clone().unwrap_or_default();
            match groups.iter_mut().find(|(key, _)| *key == extendee) {
                Some((_, indices)) => indices.push(index),
                None => groups.push((extendee, vec![index])),
            }
        }

        for (extendee, indices) in groups {
            if extendee.is_empty() {
                continue;
            }

            append_heading_space(buffer, marker);
            buffer.push_str(&format!("{levelspace}extend {extendee} {{\n"));

            for index in indices {
                let raw = raws.get(index).cloned().unwrap_or_default();
                let declaration =
                    self.build_descriptor_declaration(source, &fields[index], &raw, true);
                buffer.push_str(&format!("{levelspace}\t{declaration}\n"));
            }

            buffer.push_str(&format!("{levelspace}}}\n"));
            *marker = true;
        }
    }

    /// Writes a message.
    fn dump_descriptor(
        &mut self,
        source: &FileDescriptorProto,
        proto: &DescriptorProto,
        raw: &RawMessageDescriptor,
        buffer: &mut String,
        level: usize,
        marker: &mut bool,
    ) {
        let levelspace = "\t".repeat(level);
        let mut inner_marker = false;

        append_heading_space(buffer, marker);
        buffer.push_str(&format!(
            "{levelspace}message {} {{\n",
            proto.name.as_deref().unwrap_or("")
        ));

        let mut options = OptionList::new();
        self.dump_message_options(
            source,
            proto.options.as_ref(),
            raw.options().as_ref(),
            &mut options,
        );

        for (key, value) in options.iter() {
            append_heading_space(buffer, &mut inner_marker);
            buffer.push_str(&format!("{levelspace}\toption {key} = {value};\n"));
        }

        if !options.is_empty() {
            inner_marker = true;
        }

        if !proto.extension.is_empty() {
            self.dump_extension_descriptors(
                source,
                &proto.extension,
                raw.extensions(),
                buffer,
                level + 1,
                &mut inner_marker,
            );
        }

        let raw_nested = raw.nested_types();
        for (index, nested) in proto.nested_type.iter().enumerate() {
            let raw_nested_message = raw_nested.get(index).cloned().unwrap_or_default();
            let is_map_entry = nested
                .options
                .as_ref()
                .map(|options| options.map_entry == Some(true))
                .unwrap_or(false);

            if is_map_entry {
                continue;
            }

            self.dump_descriptor(
                source,
                nested,
                &raw_nested_message,
                buffer,
                level + 1,
                &mut inner_marker,
            );
        }

        let raw_enums = raw.enums();
        for (index, enum_type) in proto.enum_type.iter().enumerate() {
            let raw_enum = raw_enums.get(index).cloned().unwrap_or_default();
            self.dump_enum_descriptor(
                source,
                enum_type,
                &raw_enum,
                buffer,
                level + 1,
                &mut inner_marker,
            );
        }

        let raw_fields = raw.fields();
        let mut root_field_count = 0usize;
        for (index, field) in proto.field.iter().enumerate() {
            let is_root = field.oneof_index.is_none() || field.proto3_optional == Some(true);
            if !is_root {
                continue;
            }

            root_field_count += 1;
            let raw_field = raw_fields.get(index).cloned().unwrap_or_default();
            append_heading_space(buffer, &mut inner_marker);
            let declaration =
                self.build_descriptor_declaration(source, field, &raw_field, true);
            buffer.push_str(&format!("{levelspace}\t{declaration}\n"));
        }

        if root_field_count > 0 {
            inner_marker = true;
        }

        let raw_oneofs = raw.oneofs();
        for (index, oneof) in proto.oneof_decl.iter().enumerate() {
            let fields: Vec<(usize, &FieldDescriptorProto)> = proto
                .field
                .iter()
                .enumerate()
                .filter(|(_, field)| field.oneof_index == Some(index as i32))
                .collect();

            if fields
                .iter()
                .any(|(_, field)| field.proto3_optional == Some(true))
            {
                continue;
            }

            append_heading_space(buffer, &mut inner_marker);
            buffer.push_str(&format!(
                "{levelspace}\toneof {} {{\n",
                oneof.name.as_deref().unwrap_or("")
            ));

            let raw_oneof = raw_oneofs.get(index).cloned().unwrap_or_default();
            let mut oneof_options = OptionList::new();
            self.dump_oneof_options(
                source,
                oneof.options.as_ref(),
                raw_oneof.options().as_ref(),
                &mut oneof_options,
            );

            for (key, value) in oneof_options.iter() {
                buffer.push_str(&format!("{levelspace}\t\toption {key} = {value};\n"));
            }

            for (field_index, field) in fields {
                let raw_field = raw_fields.get(field_index).cloned().unwrap_or_default();
                let declaration =
                    self.build_descriptor_declaration(source, field, &raw_field, false);
                buffer.push_str(&format!("{levelspace}\t\t{declaration}\n"));
            }

            buffer.push_str(&format!("{levelspace}\t}}\n"));
            inner_marker = true;
        }

        let raw_ranges = raw.extension_ranges();
        for (index, range) in proto.extension_range.iter().enumerate() {
            append_heading_space(buffer, &mut inner_marker);

            let raw_range = raw_ranges.get(index).cloned().unwrap_or_default();
            let mut range_options = OptionList::new();
            self.dump_extension_range_options(
                source,
                range.options.as_ref(),
                raw_range.options().as_ref(),
                &mut range_options,
            );

            let parameters = format_parameters(&range_options);
            let start = range.start.unwrap_or(0);
            let end = range.end.unwrap_or(0).saturating_sub(1);

            buffer.push_str(&format!(
                "{levelspace}\textensions {}{};\n",
                format_range(start, end, MAX_FIELD_NUMBER),
                parameters
            ));
        }

        let reserved: Vec<String> = proto
            .reserved_range
            .iter()
            .map(|range| {
                format_range(
                    range.start.unwrap_or(0),
                    range.end.unwrap_or(0).saturating_sub(1),
                    MAX_FIELD_NUMBER,
                )
            })
            .collect();

        dump_reserved(
            &reserved,
            &proto.reserved_name,
            buffer,
            &levelspace,
            &mut inner_marker,
        );

        buffer.push_str(&format!("{levelspace}}}\n"));
        *marker = true;
    }

    /// Writes an enum.
    fn dump_enum_descriptor(
        &mut self,
        source: &FileDescriptorProto,
        proto: &EnumDescriptorProto,
        raw: &RawEnumDescriptor,
        buffer: &mut String,
        level: usize,
        marker: &mut bool,
    ) {
        let levelspace = "\t".repeat(level);

        append_heading_space(buffer, marker);
        buffer.push_str(&format!(
            "{levelspace}enum {} {{\n",
            proto.name.as_deref().unwrap_or("")
        ));

        let mut options = OptionList::new();
        self.dump_enum_options(
            source,
            proto.options.as_ref(),
            raw.options().as_ref(),
            &mut options,
        );

        for (key, value) in options.iter() {
            buffer.push_str(&format!("{levelspace}\toption {key} = {value};\n"));
        }

        let raw_values = raw.values();
        for (index, enum_value) in proto.value.iter().enumerate() {
            let raw_value = raw_values.get(index).cloned().unwrap_or_default();
            let mut value_options = OptionList::new();
            self.dump_enum_value_options(
                source,
                enum_value.options.as_ref(),
                raw_value.options().as_ref(),
                &mut value_options,
            );

            let parameters = format_parameters(&value_options);
            buffer.push_str(&format!(
                "{levelspace}\t{} = {}{};\n",
                enum_value.name.as_deref().unwrap_or(""),
                enum_value.number.unwrap_or(0),
                parameters
            ));
        }

        let reserved: Vec<String> = proto
            .reserved_range
            .iter()
            .map(|range| format_range(range.start.unwrap_or(0), range.end.unwrap_or(0), i32::MAX))
            .collect();

        let mut inner_marker = false;
        dump_reserved(
            &reserved,
            &proto.reserved_name,
            buffer,
            &levelspace,
            &mut inner_marker,
        );

        buffer.push_str(&format!("{levelspace}}}\n"));
        *marker = true;
    }

    /// Writes a service.
    fn dump_service(
        &mut self,
        source: &FileDescriptorProto,
        service: &ServiceDescriptorProto,
        raw: &RawServiceDescriptor,
        buffer: &mut String,
        marker: &mut bool,
    ) {
        let mut inner_marker = false;

        append_heading_space(buffer, marker);
        buffer.push_str(&format!(
            "service {} {{\n",
            service.name.as_deref().unwrap_or("")
        ));

        let mut root_options = OptionList::new();
        self.dump_service_options(
            source,
            service.options.as_ref(),
            raw.options().as_ref(),
            &mut root_options,
        );

        for (key, value) in root_options.iter() {
            buffer.push_str(&format!("\toption {key} = {value};\n"));
        }

        if !root_options.is_empty() {
            inner_marker = true;
        }

        let raw_methods = raw.methods();
        for (index, method) in service.method.iter().enumerate() {
            let client_streaming = if method.client_streaming == Some(true) {
                "stream "
            } else {
                ""
            };
            let server_streaming = if method.server_streaming == Some(true) {
                "stream "
            } else {
                ""
            };

            let declaration = format!(
                "\trpc {} ({}{}) returns ({}{})",
                method.name.as_deref().unwrap_or(""),
                client_streaming,
                method.input_type.as_deref().unwrap_or(""),
                server_streaming,
                method.output_type.as_deref().unwrap_or("")
            );

            let raw_method = raw_methods.get(index).cloned().unwrap_or_default();
            let mut options = OptionList::new();
            self.dump_method_options(
                source,
                method.options.as_ref(),
                raw_method.options().as_ref(),
                &mut options,
            );

            append_heading_space(buffer, &mut inner_marker);

            if options.is_empty() {
                buffer.push_str(&format!("{declaration};\n"));
            } else {
                buffer.push_str(&format!("{declaration} {{\n"));

                for (key, value) in options.iter() {
                    buffer.push_str(&format!("\t\toption {key} = {value};\n"));
                }

                buffer.push_str("\t}\n");
                inner_marker = true;
            }
        }

        buffer.push_str("}\n");
        *marker = true;
    }

    /// Builds the declaration of a single field.
    fn build_descriptor_declaration(
        &mut self,
        source: &FileDescriptorProto,
        field: &FieldDescriptorProto,
        raw: &RawFieldDescriptor,
        emit_field_label: bool,
    ) -> String {
        let mut type_name = resolve_type(field);
        let mut options = OptionList::new();
        let is_proto2 = source
            .syntax
            .as_deref()
            .map(|syntax| syntax.is_empty() || syntax == "proto2")
            .unwrap_or(true);

        if let Some(default_value) = &field.default_value {
            let value = match field_type(field) {
                Some(Type::String) => to_literal(default_value),
                Some(Type::Bytes) => format!("\"{default_value}\""),
                _ => default_value.clone(),
            };

            options.add("default", value);
        } else if is_proto2
            && field_type(field) == Some(Type::Enum)
            && field.label != Some(Label::Repeated as i32)
        {
            if let Some(name) = self.first_enum_value(field) {
                options.add("default", name);
            }
        }

        if let Some(json_name) = &field.json_name {
            if !json_name.is_empty() {
                options.add("json_name", to_literal(json_name));
            }
        }

        let mut field_options = OptionList::new();
        self.dump_field_options(
            source,
            field.options.as_ref(),
            raw.options().as_ref(),
            &mut field_options,
        );
        options.extend(field_options);

        let parameters = format_parameters(&options);

        let mut label: Option<&'static str> = None;
        let mut is_map = false;

        if field.label == Some(Label::Repeated as i32) && field_type(field) == Some(Type::Message) {
            if let Some(map_entry) = self.map_entry_types(field) {
                type_name = format!("map<{}, {}>", map_entry.0, map_entry.1);
                is_map = true;
            }
        }

        if !is_map && emit_field_label {
            label = if is_proto2
                || field.label == Some(Label::Repeated as i32)
                || field.proto3_optional == Some(true)
            {
                Some(get_label(field.label))
            } else {
                None
            };
        }

        let mut declaration = String::new();
        if let Some(label) = label {
            declaration.push_str(label);
            declaration.push(' ');
        }

        declaration.push_str(&format!(
            "{type_name} {} = {}{};",
            field.name.as_deref().unwrap_or(""),
            field.number.unwrap_or(0),
            parameters
        ));

        declaration
    }

    /// Returns the key and value types of a map field.
    fn map_entry_types(&self, field: &FieldDescriptorProto) -> Option<(String, String)> {
        let type_name = field.type_name.as_deref()?;
        let node = self.protobuf_type_map_index.get(type_name)?;

        let message = match &self.protobuf_type_map[*node].source {
            Some(TypeSource::Message(message)) => message,
            _ => return None,
        };

        let is_map_entry = message
            .options
            .as_ref()
            .map(|options| options.map_entry == Some(true))
            .unwrap_or(false);

        if !is_map_entry {
            return None;
        }

        let key = message.field.iter().find(|field| field.number == Some(1));
        let value = message.field.iter().find(|field| field.number == Some(2));

        Some((
            key.map(resolve_type).unwrap_or_default(),
            value.map(resolve_type).unwrap_or_default(),
        ))
    }

    /// Returns the name of the first value of the enum a field refers to.
    fn first_enum_value(&self, field: &FieldDescriptorProto) -> Option<String> {
        let type_name = field.type_name.as_deref()?;
        let node = self.protobuf_type_map_index.get(type_name)?;

        let enum_descriptor = match &self.protobuf_type_map[*node].source {
            Some(TypeSource::Enum(enum_descriptor)) => enum_descriptor,
            _ => return None,
        };

        enum_descriptor
            .value
            .first()
            .map(|value| value.name.clone().unwrap_or_default())
    }

    /// Writes the options of a file.
    #[allow(deprecated)]
    fn dump_file_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&FileOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let Some(options) = options else {
            return;
        };

        if let Some(value) = options.deprecated {
            out.add("deprecated", value.to_string());
        }
        if let Some(value) = options.optimize_for {
            out.add("optimize_for", optimize_mode_name(value));
        }
        if let Some(value) = options.cc_generic_services {
            out.add("cc_generic_services", value.to_string());
        }
        if let Some(value) = options.cc_enable_arenas {
            out.add("cc_enable_arenas", value.to_string());
        }
        if let Some(value) = &options.go_package {
            out.add("go_package", to_literal(value));
        }
        if let Some(value) = &options.java_package {
            out.add("java_package", to_literal(value));
        }
        if let Some(value) = &options.java_outer_classname {
            out.add("java_outer_classname", to_literal(value));
        }
        if let Some(value) = options.java_generate_equals_and_hash {
            out.add("java_generate_equals_and_hash", value.to_string());
        }
        if let Some(value) = options.java_generic_services {
            out.add("java_generic_services", value.to_string());
        }
        if let Some(value) = options.java_multiple_files {
            out.add("java_multiple_files", value.to_string());
        }
        if let Some(value) = options.java_string_check_utf8 {
            out.add("java_string_check_utf8", value.to_string());
        }
        if let Some(value) = options.py_generic_services {
            out.add("py_generic_services", value.to_string());
        }
        if let Some(value) = &options.ruby_package {
            out.add("ruby_package", to_literal(value));
        }
        if let Some(value) = &options.objc_class_prefix {
            out.add("objc_class_prefix", to_literal(value));
        }
        if let Some(value) = &options.csharp_namespace {
            out.add("csharp_namespace", to_literal(value));
        }
        if let Some(value) = &options.swift_prefix {
            out.add("swift_prefix", to_literal(value));
        }
        if let Some(value) = options.php_generic_services {
            out.add("php_generic_services", value.to_string());
        }
        if let Some(value) = &options.php_class_prefix {
            out.add("php_class_prefix", to_literal(value));
        }
        if let Some(value) = &options.php_namespace {
            out.add("php_namespace", to_literal(value));
        }
        if let Some(value) = &options.php_metadata_namespace {
            out.add("php_metadata_namespace", to_literal(value));
        }

        self.dump_options_matching(source, FILE_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of a field.
    fn dump_field_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&FieldOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let Some(options) = options else {
            return;
        };

        if let Some(value) = options.ctype {
            out.add("ctype", ctype_name(value));
        }
        if let Some(value) = options.deprecated {
            out.add("deprecated", value.to_string());
        }
        if let Some(value) = options.lazy {
            out.add("lazy", value.to_string());
        }
        if let Some(value) = options.packed {
            out.add("packed", value.to_string());
        }
        if let Some(value) = options.weak {
            out.add("weak", value.to_string());
        }
        if let Some(value) = options.jstype {
            out.add("jstype", jstype_name(value));
        }

        self.dump_options_matching(source, FIELD_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of a message.
    fn dump_message_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&MessageOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let Some(options) = options else {
            return;
        };

        if let Some(value) = options.message_set_wire_format {
            out.add("message_set_wire_format", value.to_string());
        }
        if let Some(value) = options.no_standard_descriptor_accessor {
            out.add("no_standard_descriptor_accessor", value.to_string());
        }
        if let Some(value) = options.deprecated {
            out.add("deprecated", value.to_string());
        }

        self.dump_options_matching(source, MESSAGE_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of an enum.
    fn dump_enum_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&EnumOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let Some(options) = options else {
            return;
        };

        if let Some(value) = options.allow_alias {
            out.add("allow_alias", value.to_string());
        }
        if let Some(value) = options.deprecated {
            out.add("deprecated", value.to_string());
        }

        self.dump_options_matching(source, ENUM_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of an enum value.
    fn dump_enum_value_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&EnumValueOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let Some(options) = options else {
            return;
        };

        if let Some(value) = options.deprecated {
            out.add("deprecated", value.to_string());
        }

        self.dump_options_matching(source, ENUM_VALUE_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of a service.
    fn dump_service_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&ServiceOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let Some(options) = options else {
            return;
        };

        if let Some(value) = options.deprecated {
            out.add("deprecated", value.to_string());
        }

        self.dump_options_matching(source, SERVICE_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of a method.
    fn dump_method_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&MethodOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let Some(options) = options else {
            return;
        };

        if let Some(value) = options.deprecated {
            out.add("deprecated", value.to_string());
        }
        if let Some(value) = options.idempotency_level {
            out.add("idempotency_level", idempotency_level_name(value));
        }

        self.dump_options_matching(source, METHOD_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of a oneof.
    fn dump_oneof_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&OneofOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        if options.is_none() {
            return;
        }

        self.dump_options_matching(source, ONEOF_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options of an extension range.
    fn dump_extension_range_options(
        &mut self,
        source: &FileDescriptorProto,
        options: Option<&ExtensionRangeOptions>,
        raw: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        if options.is_none() {
            return;
        }

        self.dump_options_matching(source, EXTENSION_RANGE_OPTIONS_TYPE, raw, out);
    }

    /// Writes the options that are declared as extensions.
    ///
    /// The options that the descriptor schema knows are written by the
    /// `dump_*_options` functions; the ones that the schema of the binary does
    /// not know are extensions and are read from the raw message.
    fn dump_options_matching(
        &mut self,
        source: &FileDescriptorProto,
        type_name: &str,
        options: Option<&RawMessage>,
        out: &mut OptionList,
    ) {
        let source_name = source.name.clone().unwrap_or_default();
        let source_index = self.protobuf_map_index.get(&source_name).copied();

        let mut dependencies: HashSet<usize> = match source_index {
            Some(index) => self.protobuf_map[index].all_public_dependencies.clone(),
            None => HashSet::new(),
        };

        if let Some(index) = source_index {
            dependencies.insert(index);
        }

        if let Some(&node_index) = self.protobuf_type_map_index.get(type_name) {
            let options_proto = match &self.protobuf_type_map[node_index].source {
                Some(TypeSource::Message(message)) => Some(message.clone()),
                _ => None,
            };

            if let Some(options_proto) = options_proto {
                let count = out.len();
                let standard = standard_option_fields(type_name);

                for field in &options_proto.field {
                    if field.name.as_deref() == Some("uninterpreted_option") {
                        continue;
                    }

                    // The options the descriptor schema knows are written by the
                    // `dump_*_options` functions; only the ones the schema of
                    // the binary does not know are read from the raw message.
                    if standard.contains(&field_number(field)) {
                        continue;
                    }

                    if let Some(options) = options {
                        self.dump_options_field_recursive(
                            field,
                            options,
                            out,
                            field.name.as_deref().unwrap_or(""),
                        );
                    }
                }

                if out.len() > count {
                    if let Some(proto_index) = self.protobuf_type_map[node_index].proto {
                        if !dependencies.contains(&proto_index) {
                            if let Some(name) = self.protobuf_map[proto_index]
                                .proto
                                .as_ref()
                                .and_then(|proto| proto.name.clone())
                            {
                                self.option_imports.insert(name);
                            }
                        }
                    }
                }
            }
        }

        let extensions: Vec<(String, FieldDescriptorProto)> = self
            .protobuf_type_map
            .iter()
            .filter_map(|node| {
                let proto_index = node.proto?;

                if !dependencies.contains(&proto_index) {
                    return None;
                }

                let field = match &node.source {
                    Some(TypeSource::Extension(field)) => field.clone(),
                    _ => return None,
                };

                if field.extendee.as_deref().unwrap_or("") != type_name {
                    return None;
                }

                Some((node.name.clone(), field))
            })
            .collect();

        for (name, field) in extensions {
            if let Some(options) = options {
                let key = format!("({})", name.trim_start_matches('.'));
                self.dump_options_field_recursive(&field, options, out, &key);
            }
        }
    }

    /// Writes a single option value, descending into message options.
    fn dump_options_field_recursive(
        &self,
        field: &FieldDescriptorProto,
        options: &RawMessage,
        out: &mut OptionList,
        key: &str,
    ) {
        if field_type(field) == Some(Type::Message) && field.label != Some(Label::Repeated as i32) {
            let extension = match options.message(field_number(field)) {
                Some(extension) => extension,
                None => return,
            };

            let count = out.len();

            for sub_field in self.message_fields_of(field.type_name.as_deref().unwrap_or("")) {
                self.dump_options_field_recursive(
                    &sub_field,
                    &extension,
                    out,
                    &format!("{key}.{}", sub_field.name.as_deref().unwrap_or("")),
                );
            }

            if out.len() == count {
                out.add(key, "{}");
            }

            return;
        }

        for value in self.get_option_values(options, field) {
            out.add(key, value);
        }
    }

    /// Returns the values of an option field.
    fn get_option_values(&self, data: &RawMessage, field: &FieldDescriptorProto) -> Vec<String> {
        let number = field_number(field);

        let values: Vec<String> = match field_type(field) {
            Some(Type::Int32) => varints(data, number)
                .into_iter()
                .map(|value| (value as u32 as i32).to_string())
                .collect(),
            Some(Type::Sint32) => varints(data, number)
                .into_iter()
                .map(|value| decode_zigzag32(value).to_string())
                .collect(),
            Some(Type::Sfixed32) => fixed32s(data, number)
                .into_iter()
                .map(|value| (value as i32).to_string())
                .collect(),
            Some(Type::Uint32) => varints(data, number)
                .into_iter()
                .map(|value| (value as u32).to_string())
                .collect(),
            Some(Type::Fixed32) => fixed32s(data, number)
                .into_iter()
                .map(|value| value.to_string())
                .collect(),
            Some(Type::Int64) => varints(data, number)
                .into_iter()
                .map(|value| (value as i64).to_string())
                .collect(),
            Some(Type::Sint64) => varints(data, number)
                .into_iter()
                .map(|value| decode_zigzag64(value).to_string())
                .collect(),
            Some(Type::Sfixed64) => fixed64s(data, number)
                .into_iter()
                .map(|value| (value as i64).to_string())
                .collect(),
            Some(Type::Uint64) => varints(data, number)
                .into_iter()
                .map(|value| value.to_string())
                .collect(),
            Some(Type::Fixed64) => fixed64s(data, number)
                .into_iter()
                .map(|value| value.to_string())
                .collect(),
            Some(Type::Bool) => varints(data, number)
                .into_iter()
                .map(|value| if value != 0 { "true" } else { "false" }.to_owned())
                .collect(),
            Some(Type::Float) => fixed32s(data, number)
                .into_iter()
                .map(|value| to_literal_float(f32::from_bits(value)))
                .collect(),
            Some(Type::Double) => fixed64s(data, number)
                .into_iter()
                .map(|value| to_literal_double(f64::from_bits(value)))
                .collect(),
            Some(Type::String) => bytes(data, number)
                .into_iter()
                .map(|value| to_literal(&String::from_utf8_lossy(value)))
                .collect(),
            Some(Type::Bytes) => bytes(data, number)
                .into_iter()
                .map(|value| to_literal_bytes(value))
                .collect(),
            Some(Type::Enum) => varints(data, number)
                .into_iter()
                .map(|value| self.get_enum_value_name(field, value as i32))
                .collect(),
            Some(Type::Message) => data
                .messages(number)
                .iter()
                .map(|message| self.build_aggregate_value(message, field))
                .collect(),
            _ => Vec::new(),
        };

        if field.label != Some(Label::Repeated as i32) && values.len() > 1 {
            let last = values.last().cloned().unwrap_or_default();
            return vec![last];
        }

        values
    }

    /// Returns the name of an enum value, or its number when it is unknown.
    fn get_enum_value_name(&self, field: &FieldDescriptorProto, number: i32) -> String {
        let type_name = field.type_name.as_deref().unwrap_or("");

        if let Some(&node) = self.protobuf_type_map_index.get(type_name) {
            if let Some(TypeSource::Enum(enum_descriptor)) = &self.protobuf_type_map[node].source {
                if let Some(value) = enum_descriptor
                    .value
                    .iter()
                    .find(|value| value.number == Some(number))
                {
                    return value.name.clone().unwrap_or_default();
                }
            }
        }

        number.to_string()
    }

    /// Builds the value of a message typed option.
    fn build_aggregate_value(&self, data: &RawMessage, field: &FieldDescriptorProto) -> String {
        let mut parts = Vec::new();

        for sub_field in self.message_fields_of(field.type_name.as_deref().unwrap_or("")) {
            for value in self.get_option_values(data, &sub_field) {
                let name = sub_field.name.as_deref().unwrap_or("");
                if field_type(&sub_field) == Some(Type::Message) {
                    parts.push(format!("{name} {value}"));
                } else {
                    parts.push(format!("{name}: {value}"));
                }
            }
        }

        if parts.is_empty() {
            "{}".to_owned()
        } else {
            format!("{{ {} }}", parts.join(" "))
        }
    }

    /// Returns the fields of the message a type name refers to.
    fn message_fields_of(&self, type_name: &str) -> Vec<FieldDescriptorProto> {
        let node = match self.protobuf_type_map_index.get(type_name) {
            Some(&node) => node,
            None => return Vec::new(),
        };

        match &self.protobuf_type_map[node].source {
            Some(TypeSource::Message(message)) => message.field.clone(),
            _ => Vec::new(),
        }
    }
}

/// Appends a blank line when the previous block was written.
fn append_heading_space(buffer: &mut String, marker: &mut bool) {
    if *marker {
        buffer.push('\n');
        *marker = false;
    }
}

/// Writes the reserved ranges and names of a message or an enum.
fn dump_reserved(
    ranges: &[String],
    names: &[String],
    buffer: &mut String,
    levelspace: &str,
    marker: &mut bool,
) {
    if !ranges.is_empty() {
        append_heading_space(buffer, marker);
        buffer.push_str(&format!("{levelspace}\treserved {};\n", ranges.join(", ")));
    }

    if !names.is_empty() {
        append_heading_space(buffer, marker);
        let literals: Vec<String> = names.iter().map(|name| to_literal(name)).collect();
        buffer.push_str(&format!("{levelspace}\treserved {};\n", literals.join(", ")));
    }
}

/// Formats the parameter list of a declaration.
fn format_parameters(options: &OptionList) -> String {
    if options.is_empty() {
        return String::new();
    }

    let parameters: Vec<String> = options
        .iter()
        .map(|(key, value)| format!("{key} = {value}"))
        .collect();

    format!(" [{}]", parameters.join(", "))
}

/// Formats a range of field numbers.
fn format_range(start: i32, end: i32, max: i32) -> String {
    if start == end {
        return format!("{start}");
    }

    if end >= max {
        return format!("{start} to max");
    }

    format!("{start} to {end}")
}

/// Joins a package and a name into a fully qualified name.
fn get_package_path(package: &str, name: &str) -> String {
    let package = if package.is_empty() || package.starts_with('.') {
        package.to_owned()
    } else {
        format!(".{package}")
    };

    if name.starts_with('.') {
        name.to_owned()
    } else {
        format!("{package}.{name}")
    }
}

/// True when the field refers to a message or an enum.
fn is_named_type(field_type: Option<i32>) -> bool {
    field_type == Some(Type::Message as i32) || field_type == Some(Type::Enum as i32)
}

/// Returns the type of a field.
fn field_type(field: &FieldDescriptorProto) -> Option<Type> {
    field.r#type.and_then(|value| Type::try_from(value).ok())
}

/// Returns the field numbers of the options that the descriptor schema knows.
///
/// These are written from the typed descriptor fields; any other option is an
/// extension and gets read from the raw message.
fn standard_option_fields(type_name: &str) -> &'static [u32] {
    match type_name {
        FILE_OPTIONS_TYPE => &[
            1,  // java_package
            8,  // java_outer_classname
            9,  // optimize_for
            10, // java_multiple_files
            11, // go_package
            16, // cc_generic_services
            17, // java_generic_services
            18, // py_generic_services
            20, // java_generate_equals_and_hash
            23, // deprecated
            27, // java_string_check_utf8
            31, // cc_enable_arenas
            36, // objc_class_prefix
            37, // csharp_namespace
            39, // swift_prefix
            40, // php_class_prefix
            41, // php_namespace
            42, // php_generic_services
            44, // php_metadata_namespace
            45, // ruby_package
        ],
        FIELD_OPTIONS_TYPE => &[
            1,  // ctype
            2,  // packed
            3,  // deprecated
            5,  // lazy
            6,  // jstype
            10, // weak
        ],
        MESSAGE_OPTIONS_TYPE => &[
            1, // message_set_wire_format
            2, // no_standard_descriptor_accessor
            3, // deprecated
            7, // map_entry
        ],
        ENUM_OPTIONS_TYPE => &[2, 3], // allow_alias, deprecated
        ENUM_VALUE_OPTIONS_TYPE => &[1], // deprecated
        SERVICE_OPTIONS_TYPE => &[33],   // deprecated
        METHOD_OPTIONS_TYPE => &[33, 34], // deprecated, idempotency_level
        _ => &[],
    }
}

/// Returns the number of a field.
fn field_number(field: &FieldDescriptorProto) -> u32 {
    u32::try_from(field.number.unwrap_or(0)).unwrap_or(0)
}

/// Returns the label of a field.
fn get_label(label: Option<i32>) -> &'static str {
    match label.and_then(|value| Label::try_from(value).ok()) {
        Some(Label::Required) => "required",
        Some(Label::Repeated) => "repeated",
        _ => "optional",
    }
}

/// Returns the protobuf type name of a field.
fn resolve_type(field: &FieldDescriptorProto) -> String {
    if is_named_type(field.r#type) {
        return field.type_name.clone().unwrap_or_default();
    }

    match field_type(field) {
        Some(Type::Int32) => "int32".to_owned(),
        Some(Type::Int64) => "int64".to_owned(),
        Some(Type::Sint32) => "sint32".to_owned(),
        Some(Type::Sint64) => "sint64".to_owned(),
        Some(Type::Uint32) => "uint32".to_owned(),
        Some(Type::Uint64) => "uint64".to_owned(),
        Some(Type::String) => "string".to_owned(),
        Some(Type::Bool) => "bool".to_owned(),
        Some(Type::Bytes) => "bytes".to_owned(),
        Some(Type::Double) => "double".to_owned(),
        Some(Type::Enum) => "enum".to_owned(),
        Some(Type::Float) => "float".to_owned(),
        Some(Type::Group) => "GROUP".to_owned(),
        Some(Type::Message) => "message".to_owned(),
        Some(Type::Fixed32) => "fixed32".to_owned(),
        Some(Type::Fixed64) => "fixed64".to_owned(),
        Some(Type::Sfixed32) => "sfixed32".to_owned(),
        Some(Type::Sfixed64) => "sfixed64".to_owned(),
        None => field.r#type.unwrap_or(0).to_string(),
    }
}

/// Returns the name of an `OptimizeMode` value.
fn optimize_mode_name(value: i32) -> String {
    match value {
        1 => "SPEED".to_owned(),
        2 => "CODE_SIZE".to_owned(),
        3 => "LITE_RUNTIME".to_owned(),
        other => other.to_string(),
    }
}

/// Returns the name of a `CType` value.
fn ctype_name(value: i32) -> String {
    match value {
        0 => "STRING".to_owned(),
        1 => "CORD".to_owned(),
        2 => "STRING_PIECE".to_owned(),
        other => other.to_string(),
    }
}

/// Returns the name of a `JsType` value.
fn jstype_name(value: i32) -> String {
    match value {
        0 => "JS_NORMAL".to_owned(),
        1 => "JS_STRING".to_owned(),
        2 => "JS_NUMBER".to_owned(),
        other => other.to_string(),
    }
}

/// Returns the name of an `IdempotencyLevel` value.
fn idempotency_level_name(value: i32) -> String {
    match value {
        0 => "IDEMPOTENCY_UNKNOWN".to_owned(),
        1 => "NO_SIDE_EFFECTS".to_owned(),
        2 => "IDEMPOTENT".to_owned(),
        other => other.to_string(),
    }
}

/// Returns the varint values of a field.
fn varints(data: &RawMessage, number: u32) -> Vec<u64> {
    data.values(number)
        .into_iter()
        .filter_map(|value| value.as_varint())
        .collect()
}

/// Returns the 32 bit values of a field.
fn fixed32s(data: &RawMessage, number: u32) -> Vec<u32> {
    data.values(number)
        .into_iter()
        .filter_map(|value| value.as_fixed32())
        .collect()
}

/// Returns the 64 bit values of a field.
fn fixed64s(data: &RawMessage, number: u32) -> Vec<u64> {
    data.values(number)
        .into_iter()
        .filter_map(|value| value.as_fixed64())
        .collect()
}

/// Returns the length delimited values of a field.
fn bytes(data: &RawMessage, number: u32) -> Vec<&[u8]> {
    data.values(number)
        .into_iter()
        .filter_map(|value| value.as_bytes())
        .collect()
}

/// Decodes a zig zag encoded 32 bit value.
fn decode_zigzag32(value: u64) -> i32 {
    let value = value as u32;
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

/// Decodes a zig zag encoded 64 bit value.
fn decode_zigzag64(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}
