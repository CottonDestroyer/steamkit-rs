//! Rust code generator for the SteamLanguage definition language.
//!
//! * a member marked `removed` is dropped;
//! * a member marked `obsolete` is kept and marked `#[deprecated]`;
//! * an enum member without an explicit value continues from the previous
//!   member;
//! * a `|` separated value is the bitwise or of its terms, where a term is a
//!   literal or an earlier member of the same enum;
//! * an enum's underlying type is the declared one (`byte`, `uint`, ...) or
//!   `int` by default.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::analyzer::{ClassNode, EnumNode, Member, Node, Root, TypeRef};
use crate::error::{LangError, LangResult};

/// The Rust representation of an enum's underlying type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Underlying {
    /// The Rust primitive type.
    pub rust: &'static str,
    /// The size in bytes.
    pub size: usize,
    /// True when the type is unsigned.
    pub unsigned: bool,
}

impl Underlying {
    /// Maps a `.steamd` type name (`byte`, `uint`, ...) to its Rust type.
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "byte" => Underlying {
                rust: "u8",
                size: 1,
                unsigned: true,
            },
            "short" => Underlying {
                rust: "i16",
                size: 2,
                unsigned: false,
            },
            "ushort" => Underlying {
                rust: "u16",
                size: 2,
                unsigned: true,
            },
            "int" => Underlying {
                rust: "i32",
                size: 4,
                unsigned: false,
            },
            "uint" => Underlying {
                rust: "u32",
                size: 4,
                unsigned: true,
            },
            "long" => Underlying {
                rust: "i64",
                size: 8,
                unsigned: false,
            },
            "ulong" => Underlying {
                rust: "u64",
                size: 8,
                unsigned: true,
            },
            _ => return None,
        })
    }

    /// The underlying type used when an enum doesn't declare one (`int`).
    const DEFAULT: Underlying = Underlying {
        rust: "i32",
        size: 4,
        unsigned: false,
    };

    /// The inclusive value range of the type.
    fn range(self) -> (i128, i128) {
        match self.rust {
            "u8" => (0, u8::MAX as i128),
            "u16" => (0, u16::MAX as i128),
            "u32" => (0, u32::MAX as i128),
            "u64" => (0, u64::MAX as i128),
            "i16" => (i16::MIN as i128, i16::MAX as i128),
            "i32" => (i32::MIN as i128, i32::MAX as i128),
            "i64" => (i64::MIN as i128, i64::MAX as i128),
            _ => (i128::MIN, i128::MAX),
        }
    }
}

/// A member of an enum with its resolved numeric value.
struct ResolvedMember {
    name: String,
    value: i128,
    obsolete: Option<String>,
    /// True for the first member with this value. Rust doesn't allow
    /// duplicate discriminants (`E0081`), so later members with the same
    /// value become associated constants instead of variants.
    primary: bool,
}

/// A resolved constant of a class.
struct ResolvedConst {
    rust_name: String,
    type_name: String,
    value: i128,
}

/// The generator state.
pub struct Generator {
    /// The merged root node of the definition files.
    root: Root,
    /// The label used for the `Source:` doc comments.
    source_label: String,
}

impl Generator {
    /// Creates a generator for the given root node.
    pub fn new(root: Root, source_label: impl Into<String>) -> Self {
        Generator {
            root,
            source_label: source_label.into(),
        }
    }

    /// Generates the complete Rust source file.
    pub fn generate(&self) -> LangResult<String> {
        let enums: Vec<&EnumNode> = self
            .root
            .children
            .iter()
            .filter_map(|node| match node {
                Node::Enum(node) => Some(node),
                Node::Class(_) => None,
            })
            .collect();

        let classes: Vec<&ClassNode> = self
            .root
            .children
            .iter()
            .filter_map(|node| match node {
                Node::Class(node) => Some(node),
                Node::Enum(_) => None,
            })
            .collect();

        // A global table of literal member values, used to resolve a value
        // term that refers to a member of a different enum.
        let mut literals: HashMap<String, i128> = HashMap::new();
        for node in &enums {
            for member in node.members.iter().filter(|member| member.is_emitted()) {
                if member.values.len() == 1 {
                    if let Some(value) = parse_integer(&member.values[0]) {
                        literals.insert(member.name.clone(), value);
                    }
                }
            }
        }

        // Resolve every enum.
        let mut resolved_enums: HashMap<String, Vec<ResolvedMember>> = HashMap::new();
        for node in &enums {
            let resolved = self.resolve_enum(node, &literals)?;
            resolved_enums.insert(node.name.clone(), resolved);
        }

        // Resolve every class constant.
        let consts = self.resolve_all_consts(&classes)?;

        let mut out = String::new();
        self.emit_header(&mut out);

        for node in &enums {
            let resolved = resolved_enums
                .get(&node.name)
                .expect("every enum was resolved above");
            self.emit_enum(&mut out, node, resolved)?;
        }

        if !classes.is_empty() {
            out.push('\n');
            self.emit_structs(&mut out, &classes, &consts, &resolved_enums)?;
        }

        Ok(out)
    }

    fn emit_header(&self, out: &mut String) {
        out.push_str("// <auto-generated/>\n");
        out.push_str("//\n");
        out.push_str("// Generated by `steamkit-lang-gen` (crate `steamkit-tools`) from the Valve\n");
        out.push_str("// SteamLanguage definition files in `crates/steamkit-tools/lang/`.\n");
        out.push_str("//\n");
        out.push_str("// Do not edit by hand - re-run the generator instead:\n");
        out.push_str("//\n");
        out.push_str("// ```text\n");
        out.push_str("// cargo run -p steamkit-tools --bin steamkit-lang-gen\n");
        out.push_str("// ```\n");
        out.push_str("//\n");
        out.push_str("// Rules:\n");
        out.push_str("//\n");
        out.push_str("// * members marked `removed` are not emitted;\n");
        out.push_str("// * members marked `obsolete` are emitted and marked `#[deprecated]`;\n");
        out.push_str("// * an enum member without an explicit value continues from the previous one;\n");
        out.push_str("// * a `|` separated value is the bitwise or of its terms.\n");
        out.push('\n');
        out.push_str("#![allow(non_camel_case_types)]\n");
        out.push('\n');
        self.emit_error_type(out);
    }

    fn emit_error_type(&self, out: &mut String) {
        out.push_str("/// Error returned when a numeric value does not match any variant of an enum.\n");
        out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n");
        out.push_str("pub struct UnknownEnumValue {\n");
        out.push_str("    /// The name of the enum the value was looked up in.\n");
        out.push_str("    pub enum_name: &'static str,\n");
        out.push_str("    /// The value that has no matching variant.\n");
        out.push_str("    pub value: i128,\n");
        out.push_str("}\n");
        out.push('\n');
        out.push_str("impl UnknownEnumValue {\n");
        out.push_str("    /// Creates a new error for the given enum and value.\n");
        out.push_str("    pub const fn new(enum_name: &'static str, value: i128) -> Self {\n");
        out.push_str("        UnknownEnumValue { enum_name, value }\n");
        out.push_str("    }\n");
        out.push_str("}\n");
        out.push('\n');
        out.push_str("impl std::fmt::Display for UnknownEnumValue {\n");
        out.push_str("    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n");
        out.push_str("        write!(f, \"{} is not a known value of {}\", self.value, self.enum_name)\n");
        out.push_str("    }\n");
        out.push_str("}\n");
        out.push('\n');
        out.push_str("impl std::error::Error for UnknownEnumValue {}\n");
        out.push('\n');
        out.push_str("impl From<UnknownEnumValue> for crate::error::SteamError {\n");
        out.push_str("    fn from(value: UnknownEnumValue) -> Self {\n");
        out.push_str("        crate::error::SteamError::InvalidArgument(value.to_string())\n");
        out.push_str("    }\n");
        out.push_str("}\n");
        out.push('\n');
    }

    // ---------------------------------------------------------------- enums

    fn resolve_enum(
        &self,
        node: &EnumNode,
        literals: &HashMap<String, i128>,
    ) -> LangResult<Vec<ResolvedMember>> {
        let underlying = self.underlying_of(node)?;
        let (low, high) = underlying.range();

        let mut table: HashMap<String, i128> = HashMap::new();
        let mut resolved: Vec<ResolvedMember> = Vec::new();
        let mut previous: Option<i128> = None;
        let mut seen_values: Vec<i128> = Vec::new();

        for member in &node.members {
            if !member.is_emitted() {
                // Removed members don't take part in the auto increment
                // either.
                continue;
            }

            let value = if member.values.is_empty() {
                previous.map(|value| value + 1).unwrap_or(0)
            } else {
                self.eval_expression(member, &table, literals)?
            };

            if value < low || value > high {
                return Err(LangError::unsupported(
                    &self.source_label,
                    format!(
                        "enum '{}': member '{}' has value {} which does not fit into '{}'",
                        node.name, member.name, value, underlying.rust
                    ),
                ));
            }

            let primary = !seen_values.contains(&value);
            if primary {
                seen_values.push(value);
            }

            table.insert(member.name.clone(), value);
            previous = Some(value);
            resolved.push(ResolvedMember {
                name: member.name.clone(),
                value,
                obsolete: member.obsolete.clone(),
                primary,
            });
        }

        Ok(resolved)
    }

    fn underlying_of(&self, node: &EnumNode) -> LangResult<Underlying> {
        match &node.underlying {
            Some(name) => Underlying::from_name(name).ok_or_else(|| {
                LangError::unsupported(
                    &self.source_label,
                    format!("enum '{}': unsupported underlying type '{}'", node.name, name),
                )
            }),
            None => Ok(Underlying::DEFAULT),
        }
    }

    fn eval_expression(
        &self,
        member: &Member,
        table: &HashMap<String, i128>,
        literals: &HashMap<String, i128>,
    ) -> LangResult<i128> {
        let mut result: i128 = 0;
        for term in &member.values {
            let value = if let Some(literal) = parse_integer(term) {
                literal
            } else if let Some(value) = table.get(term) {
                *value
            } else if let Some(value) = literals.get(term) {
                *value
            } else {
                return Err(LangError::unsupported(
                    &self.source_label,
                    format!("member '{}': unknown value term '{}'", member.name, term),
                ));
            };
            result |= value;
        }
        Ok(result)
    }

    fn emit_enum(
        &self,
        out: &mut String,
        node: &EnumNode,
        resolved: &[ResolvedMember],
    ) -> LangResult<()> {
        let underlying = self.underlying_of(node)?;

        let _ = writeln!(out, "/// The `{}` enumeration.", node.name);
        if node.flags {
            let _ = writeln!(
                out,
                "///\n/// A bit flag set: the variants can be combined with a bitwise or."
            );
        }
        let _ = writeln!(out, "///");
        let _ = writeln!(out, "/// Source: `{}`.", self.source_label);
        let _ = writeln!(
            out,
            "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]"
        );
        let _ = writeln!(out, "#[repr({})]", underlying.rust);
        let _ = writeln!(out, "pub enum {} {{", node.name);

        for member in resolved {
            if !member.primary {
                continue;
            }
            if let Some(reason) = &member.obsolete {
                if reason.is_empty() {
                    let _ = writeln!(out, "    #[deprecated]");
                } else {
                    let _ = writeln!(out, "    #[deprecated = \"{}\"]", escape_string(reason));
                }
            }
            let _ = writeln!(
                out,
                "    {} = {},",
                rust_variant_name(&member.name),
                literal_for(member.value, underlying)
            );
        }

        let _ = writeln!(out, "}}\n");

        self.emit_enum_impl(out, node, resolved, underlying);
        self.emit_enum_conversions(out, node, resolved, underlying);

        Ok(())
    }

    fn emit_enum_impl(
        &self,
        out: &mut String,
        node: &EnumNode,
        resolved: &[ResolvedMember],
        underlying: Underlying,
    ) {
        let _ = writeln!(out, "#[allow(deprecated)]");
        let _ = writeln!(out, "impl {} {{", node.name);

                // Members that share their value with an earlier member can't
                // be variants (`E0081`), so they become associated constants.
        for member in resolved {
            if member.primary {
                continue;
            }
            let Some(primary) = resolved
                .iter()
                .find(|other| other.primary && other.value == member.value)
            else {
                continue;
            };
            let _ = writeln!(
                out,
                "    /// Alias of [`{}::{}`].",
                node.name,
                rust_variant_name(&primary.name)
            );
            let _ = writeln!(
                out,
                "    pub const {}: {} = {}::{};",
                rust_variant_name(&member.name),
                node.name,
                node.name,
                rust_variant_name(&primary.name)
            );
        }
        if resolved.iter().any(|member| !member.primary) {
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "    /// Every variant of `{}`.", node.name);
        let _ = writeln!(out, "    pub const ALL: &'static [{}] = &[", node.name);
        for member in resolved {
            let _ = writeln!(
                out,
                "        {}::{},",
                node.name,
                rust_variant_name(&member.name)
            );
        }
        let _ = writeln!(out, "    ];\n");
        let _ = writeln!(
            out,
            "    /// The numeric value of this variant.\n    pub const fn value(self) -> {} {{",
            underlying.rust
        );
        let _ = writeln!(out, "        self as {}", underlying.rust);
        let _ = writeln!(out, "    }}\n");

        if node.flags {
            let _ = writeln!(out, "    /// The raw bit pattern of this flag set.");
            let _ = writeln!(out, "    pub const fn bits(self) -> {} {{", underlying.rust);
            let _ = writeln!(out, "        self as {}", underlying.rust);
            let _ = writeln!(out, "    }}\n");
            let _ = writeln!(out, "    /// True when no bit of this flag set is set.");
            let _ = writeln!(out, "    pub const fn is_empty(self) -> bool {{");
            let _ = writeln!(out, "        (self as {}) == 0", underlying.rust);
            let _ = writeln!(out, "    }}\n");
            let _ = writeln!(
                out,
                "    /// True when every bit of `other` is set in this flag set."
            );
            let _ = writeln!(out, "    pub const fn contains(self, other: Self) -> bool {{");
            let _ = writeln!(
                out,
                "        (self as {} & other as {}) == (other as {})",
                underlying.rust, underlying.rust, underlying.rust
            );
            let _ = writeln!(out, "    }}\n");
            let _ = writeln!(
                out,
                "    /// The bitwise or of two flag sets, as a raw value."
            );
            let _ = writeln!(
                out,
                "    pub const fn union(self, other: Self) -> {} {{",
                underlying.rust
            );
            let _ = writeln!(
                out,
                "        (self as {}) | (other as {})",
                underlying.rust, underlying.rust
            );
            let _ = writeln!(out, "    }}\n");
            let _ = writeln!(
                out,
                "    /// The bitwise and of two flag sets, as a raw value."
            );
            let _ = writeln!(
                out,
                "    pub const fn intersect(self, other: Self) -> {} {{",
                underlying.rust
            );
            let _ = writeln!(
                out,
                "        (self as {}) & (other as {})",
                underlying.rust, underlying.rust
            );
            let _ = writeln!(out, "    }}\n");
        }

        let _ = writeln!(
            out,
            "    /// Builds a variant from its numeric value.\n    ///\n    /// When several variants share a value, the first declared one is returned."
        );
        let _ = writeln!(
            out,
            "    pub fn from_value(value: {}) -> Option<Self> {{",
            underlying.rust
        );
        let _ = writeln!(out, "        match value {{");
        for member in resolved {
            if !member.primary {
                continue;
            }
            let _ = writeln!(
                out,
                "            {} => Some({}::{}),",
                literal_for(member.value, underlying),
                node.name,
                rust_variant_name(&member.name)
            );
        }
        let _ = writeln!(out, "            _ => None,");
        let _ = writeln!(out, "        }}");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}\n");
    }

    fn emit_enum_conversions(
        &self,
        out: &mut String,
        node: &EnumNode,
        resolved: &[ResolvedMember],
        underlying: Underlying,
    ) {
        // `TryFrom` for the underlying type.
        let _ = writeln!(out, "impl TryFrom<{}> for {} {{", underlying.rust, node.name);
        let _ = writeln!(out, "    type Error = UnknownEnumValue;\n");
        let _ = writeln!(
            out,
            "    fn try_from(value: {}) -> Result<Self, UnknownEnumValue> {{",
            underlying.rust
        );
        let _ = writeln!(
            out,
            "        Self::from_value(value).ok_or(UnknownEnumValue::new(\"{}\", value as i128))",
            node.name
        );
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}\n");

        // `TryFrom<u32>` - the wire representation of a message id.
        let all_non_negative = resolved.iter().all(|member| member.value >= 0);
        let fits_u32 = resolved.iter().all(|member| member.value <= u32::MAX as i128);
        if all_non_negative && fits_u32 && underlying.rust != "u32" {
            let _ = writeln!(out, "#[allow(deprecated)]");
            let _ = writeln!(out, "impl TryFrom<u32> for {} {{", node.name);
            let _ = writeln!(out, "    type Error = UnknownEnumValue;\n");
            let _ = writeln!(
                out,
                "    fn try_from(value: u32) -> Result<Self, UnknownEnumValue> {{"
            );
            let _ = writeln!(out, "        match value {{");
            for member in resolved {
                if !member.primary {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "            {} => Some({}::{}),",
                    member.value,
                    node.name,
                    rust_variant_name(&member.name)
                );
            }
            let _ = writeln!(out, "            _ => None,");
            let _ = writeln!(out, "        }}");
            let _ = writeln!(
                out,
                "        .ok_or(UnknownEnumValue::new(\"{}\", value as i128))",
                node.name
            );
            let _ = writeln!(out, "    }}");
            let _ = writeln!(out, "}}\n");
        }

        // `From` into the underlying type.
        let _ = writeln!(out, "impl From<{}> for {} {{", node.name, underlying.rust);
        let _ = writeln!(
            out,
            "    fn from(value: {}) -> {} {{",
            node.name, underlying.rust
        );
        let _ = writeln!(out, "        value as {}", underlying.rust);
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}\n");
    }

    // -------------------------------------------------------------- structs

    fn resolve_all_consts(
        &self,
        classes: &[&ClassNode],
    ) -> LangResult<HashMap<String, ResolvedConst>> {
        let mut consts: HashMap<String, ResolvedConst> = HashMap::new();

        // First pass: every constant whose value is a literal.
        for class in classes {
            for member in &class.members {
                if member.flags.as_deref() != Some("const") || !member.is_emitted() {
                    continue;
                }
                if member.values.len() != 1 {
                    continue;
                }
                let Some(value) = parse_integer(&member.values[0]) else {
                    continue;
                };
                consts.insert(
                    format!("{}::{}", class.name, member.name),
                    ResolvedConst {
                        rust_name: to_screaming_snake_case(&member.name),
                        type_name: self.rust_type_of(member.type_ref.as_ref()),
                        value,
                    },
                );
            }
        }

        // Second pass: every constant that refers to another constant.
        for class in classes {
            for member in &class.members {
                if member.flags.as_deref() != Some("const") || !member.is_emitted() {
                    continue;
                }
                let key = format!("{}::{}", class.name, member.name);
                if consts.contains_key(&key) {
                    continue;
                }
                if member.values.len() != 1 {
                    return Err(LangError::unsupported(
                        &self.source_label,
                        format!("constant '{}' must have exactly one value", member.name),
                    ));
                }
                let term = &member.values[0];
                let Some(found) = consts.get(term) else {
                    return Err(LangError::unsupported(
                        &self.source_label,
                        format!("constant '{}': cannot resolve value '{}'", member.name, term),
                    ));
                };
                consts.insert(
                    key,
                    ResolvedConst {
                        rust_name: to_screaming_snake_case(&member.name),
                        type_name: self.rust_type_of(member.type_ref.as_ref()),
                        value: found.value,
                    },
                );
            }
        }

        Ok(consts)
    }

    fn emit_structs(
        &self,
        out: &mut String,
        classes: &[&ClassNode],
        consts: &HashMap<String, ResolvedConst>,
        resolved_enums: &HashMap<String, Vec<ResolvedMember>>,
    ) -> LangResult<()> {
        let _ = writeln!(
            out,
            "/// The message and header structures declared in the Valve definition files.\n///\n/// They live in a nested module so the enums above stay at the root of\n/// `steam_language`."
        );
        let _ = writeln!(out, "pub mod internal {{");
        let _ = writeln!(out, "    #![allow(non_camel_case_types)]\n");

        // Import every generated enum that is used by a field.
        let mut names: Vec<String> = Vec::new();
        for class in classes {
            if class.removed {
                continue;
            }
            for member in &class.members {
                let Some(type_ref) = &member.type_ref else {
                    continue;
                };
                if type_ref.is_local
                    && resolved_enums.contains_key(&type_ref.name)
                    && !names.contains(&type_ref.name)
                {
                    names.push(type_ref.name.clone());
                }
            }
        }
        if !names.is_empty() {
            names.sort();
            let _ = writeln!(out, "    use super::{{");
            for name in &names {
                let _ = writeln!(out, "        {},", name);
            }
            let _ = writeln!(out, "    }};\n");
        }

        for class in classes {
            if class.removed {
                continue;
            }
            self.emit_struct(out, class, consts, resolved_enums)?;
        }

        let _ = writeln!(out, "}}");
        Ok(())
    }

    fn emit_struct(
        &self,
        out: &mut String,
        class: &ClassNode,
        consts: &HashMap<String, ResolvedConst>,
        resolved_enums: &HashMap<String, Vec<ResolvedMember>>,
    ) -> LangResult<()> {
        let _ = writeln!(out, "    /// The `{}` structure.", class.name);
        if let Some(ident) = &class.ident {
            if let Some(member) = &ident.member {
                let _ = writeln!(out, "    ///");
                let _ = writeln!(
                    out,
                    "    /// Carries the `{}::{}` message id.",
                    ident.name, member
                );
            }
        }
        let _ = writeln!(out, "    ///");
        let _ = writeln!(out, "    /// Source: `{}`.", self.source_label);

        let mut fields: Vec<(String, String, Member)> = Vec::new();
        for member in &class.members {
            if !member.is_emitted() {
                continue;
            }
            if member.flags.as_deref() == Some("const") {
                // Constants are emitted as associated constants below.
                continue;
            }
            let rust_type = self.rust_field_type(member);
            let field_name = rust_field_name(&member.name);
            fields.push((field_name, rust_type, member.clone()));
        }

        let copy = fields
            .iter()
            .all(|(_, rust_type, _)| !rust_type.starts_with("Vec<") && rust_type != "String");

        if copy {
            let _ = writeln!(out, "    #[derive(Clone, Copy, Debug, PartialEq, Eq)]");
        } else {
            let _ = writeln!(out, "    #[derive(Clone, Debug, PartialEq, Eq)]");
        }
        let _ = writeln!(out, "    pub struct {} {{", class.name);

        for (field_name, rust_type, _) in &fields {
            let _ = writeln!(out, "        pub {}: {},", field_name, rust_type);
        }

        let _ = writeln!(out, "    }}\n");

        // Associated constants.
        let mut has_consts = false;
        for member in &class.members {
            if !member.is_emitted() || member.flags.as_deref() != Some("const") {
                continue;
            }
            if !has_consts {
                let _ = writeln!(out, "    impl {} {{", class.name);
                has_consts = true;
            }
            let key = format!("{}::{}", class.name, member.name);
            let Some(found) = consts.get(&key) else {
                continue;
            };
            let _ = writeln!(
                out,
                "        /// The `{}` constant of `{}`.",
                member.name, class.name
            );
            let _ = writeln!(
                out,
                "        pub const {}: {} = {};",
                found.rust_name,
                found.type_name,
                literal_for(found.value, Underlying::DEFAULT)
            );
        }
        if has_consts {
            let _ = writeln!(out, "    }}\n");
        }

        // `Default` implementation.
        let _ = writeln!(out, "    impl Default for {} {{", class.name);
        let _ = writeln!(out, "        fn default() -> Self {{");
        let _ = writeln!(out, "            Self {{");
        for (field_name, rust_type, member) in &fields {
            let default = self.default_value(member, rust_type, consts, resolved_enums);
            let _ = writeln!(out, "                {}: {},", field_name, default);
        }
        let _ = writeln!(out, "            }}");
        let _ = writeln!(out, "        }}");
        let _ = writeln!(out, "    }}\n");

        Ok(())
    }

    /// The Rust type of a member.
    fn rust_type_of(&self, type_ref: Option<&TypeRef>) -> String {
        match type_ref {
            Some(type_ref) if type_ref.is_local => type_ref.name.clone(),
            Some(type_ref) => primitive_type(&type_ref.name),
            None => "u8".to_owned(),
        }
    }

    /// The Rust type of a struct field, taking the marshal flags into account.
    fn rust_field_type(&self, member: &Member) -> String {
        let flags = member.flags.as_deref().unwrap_or("");

        // A protobuf payload is kept as its serialized bytes.
        if flags == "proto" {
            return "Vec<u8>".to_owned();
        }
        if flags == "boolmarshal" {
            return "bool".to_owned();
        }
        if flags == "steamidmarshal" || flags == "gameidmarshal" {
            return "u64".to_owned();
        }

        let base = self.rust_type_of(member.type_ref.as_ref());

        // `byte<20> loginKey` - a fixed size array becomes a `Vec`.
        if let Some(flags_opt) = &member.flags_opt {
            if flags_opt.parse::<i64>().is_ok() {
                return format!("Vec<{}>", base);
            }
        }

        base
    }

    /// The default value of a struct field.
    fn default_value(
        &self,
        member: &Member,
        rust_type: &str,
        consts: &HashMap<String, ResolvedConst>,
        resolved_enums: &HashMap<String, Vec<ResolvedMember>>,
    ) -> String {
        let flags = member.flags.as_deref().unwrap_or("");

        if flags == "boolmarshal" {
            return "false".to_owned();
        }

        if rust_type.starts_with("Vec<") {
            return "Vec::new()".to_owned();
        }

        let Some(term) = member.values.first().cloned() else {
            // No explicit default means zero. For an enum that's the variant
            // with the value zero, or else the first declared variant.
            if let Some(variants) = resolved_enums.get(rust_type) {
                if let Some(zero) = variants.iter().find(|variant| variant.value == 0) {
                    return format!("{}::{}", rust_type, rust_variant_name(&zero.name));
                }
                if let Some(first) = variants.first() {
                    return format!("{}::{}", rust_type, rust_variant_name(&first.name));
                }
            }
            if rust_type == "bool" {
                return "false".to_owned();
            }
            return "0".to_owned();
        };

        // `ulong.MaxValue`
        if let Some(value) = primitive_special_value(&term) {
            return value.to_owned();
        }

        // `Enum::Member` or `Class::CONST`
        if let Some((scope, member_name)) = term.split_once("::") {
            if let Some(type_ref) = &member.type_ref {
                if type_ref.is_local && type_ref.name == scope {
                    return format!("{}::{}", scope, rust_variant_name(member_name));
                }
            }
            if let Some(found) = consts.get(&term) {
                return literal_for(found.value, Underlying::DEFAULT);
            }
            if resolved_enums.contains_key(scope) {
                return format!("{}::{}", scope, rust_variant_name(member_name));
            }
            return "0".to_owned();
        }

        if let Some(literal) = parse_integer(&term) {
            return literal_for(literal, Underlying::DEFAULT);
        }

        "0".to_owned()
    }
}

/// Parses a decimal or hexadecimal integer literal.
fn parse_integer(text: &str) -> Option<i128> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let value = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        i128::from_str_radix(hex, 16).ok()?
    } else {
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        digits.parse::<i128>().ok()?
    };
    Some(if negative { -value } else { value })
}

/// The Rust literal for a value of the given underlying type.
fn literal_for(value: i128, underlying: Underlying) -> String {
    match underlying.rust {
        "u8" | "u16" | "u32" | "u64" => format!("{}", value as u64),
        _ => format!("{}", value),
    }
}

/// Maps a `.steamd` primitive type name to its Rust type.
fn primitive_type(name: &str) -> String {
    match name {
        "byte" => "u8".to_owned(),
        "short" => "i16".to_owned(),
        "ushort" => "u16".to_owned(),
        "int" => "i32".to_owned(),
        "uint" => "u32".to_owned(),
        "long" => "i64".to_owned(),
        "ulong" => "u64".to_owned(),
        "char" => "char".to_owned(),
        "bool" => "bool".to_owned(),
        other => other.to_owned(),
    }
}

/// The value of a well known primitive expression such as `ulong.MaxValue`.
fn primitive_special_value(term: &str) -> Option<&'static str> {
    match term {
        "ulong.MaxValue" => Some("u64::MAX"),
        "uint.MaxValue" => Some("u32::MAX"),
        "ushort.MaxValue" => Some("u16::MAX"),
        "byte.MaxValue" => Some("u8::MAX"),
        "long.MaxValue" => Some("i64::MAX"),
        "int.MaxValue" => Some("i32::MAX"),
        "long.MinValue" => Some("i64::MIN"),
        "int.MinValue" => Some("i32::MIN"),
        _ => None,
    }
}

/// Rust keywords that must be written as raw identifiers.
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

/// The Rust name of an enum variant, using a raw identifier for keywords.
pub fn rust_variant_name(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{}", name)
    } else {
        name.to_owned()
    }
}

/// The Rust name of a struct field, using a raw identifier for keywords.
pub fn rust_field_name(name: &str) -> String {
    let snake = to_snake_case(name);
    if RUST_KEYWORDS.contains(&snake.as_str()) {
        format!("r#{}", snake)
    } else {
        snake
    }
}

/// Escapes a string so that it can be used inside a Rust string literal.
fn escape_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Converts a name to `snake_case`.
pub fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut previous_lower = false;
    for (index, character) in name.chars().enumerate() {
        if character == '_' {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            out.push('_');
            previous_lower = false;
            continue;
        }
        if character.is_ascii_uppercase() {
            let previous_is_digit = name
                .chars()
                .nth(index.saturating_sub(1))
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false);
            if index > 0 && (previous_lower || previous_is_digit) && !out.ends_with('_') {
                out.push('_');
            }
            out.push(character.to_ascii_lowercase());
            previous_lower = false;
        } else {
            out.push(character);
            previous_lower = character.is_ascii_lowercase() || character.is_ascii_digit();
        }
    }
    out
}

/// Converts a name to `SCREAMING_SNAKE_CASE`.
pub fn to_screaming_snake_case(name: &str) -> String {
    to_snake_case(name).to_ascii_uppercase()
}
