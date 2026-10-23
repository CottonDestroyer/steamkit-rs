//! Parser for the SteamLanguage definition language.
//!
//! Supported grammar:
//!
//! ```text
//! #import "other.steamd"
//! enum NAME { ... }
//! enum NAME<TYPE> { ... }
//! enum NAME flags { ... }
//! class NAME { ... }
//! class NAME<Enum::Member> { ... }
//! ```
//!
//! Members are `NAME;`, `TYPE NAME;` or `TYPE NAME = value;` where `value`
//! may be a `|` separated expression. After the `;` a member may be marked
//! `obsolete` or `removed`, each with an optional quoted reason.
//!
//! Notes:
//!
//! * `removed` members are dropped, `obsolete` ones are kept;
//! * an imported file is parsed into its own root first, and its children are
//!   then spliced into the importing root;
//! * a member without a value continues from the previous member.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{LangError, LangResult};
use crate::tokenizer::{error_at, tokenize, Token, TokenKind};

/// A reference to a type used by a member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeRef {
    /// The type as the generator should spell it.
    pub name: String,
    /// True when the type is defined in the `.steamd` files (an enum or a
    /// class) and therefore has a generated Rust counterpart.
    pub is_local: bool,
    /// For `Enum::Member` references, the referenced member.
    pub member: Option<String>,
}

impl TypeRef {
    /// A weak (built-in or unknown) type reference.
    fn weak(name: &str) -> Self {
        TypeRef {
            name: name.to_owned(),
            is_local: false,
            member: None,
        }
    }

    /// A reference to a type declared in the definition files.
    fn local(name: &str, member: Option<String>) -> Self {
        TypeRef {
            name: name.to_owned(),
            is_local: true,
            member,
        }
    }
}

/// A single member of an enum or a class.
#[derive(Clone, Debug)]
pub struct Member {
    /// The member name, exactly as written in the `.steamd` file.
    pub name: String,
    /// The declared type of the member, if any.
    pub type_ref: Option<TypeRef>,
    /// The marshal flag (`const`, `steamidmarshal`, `proto`, `protomask`, ...).
    pub flags: Option<String>,
    /// The optional argument of the flag (`byte<20>` -> `20`).
    pub flags_opt: Option<String>,
    /// The raw value terms, split on `|`.
    pub values: Vec<String>,
    /// `Some(reason)` when the member is marked `obsolete`; an empty reason
    /// means "obsolete without a message".
    pub obsolete: Option<String>,
    /// True when the member is marked `removed`; such members are not emitted.
    pub removed: bool,
}

impl Member {
    /// True when the member should be part of the generated code.
    pub fn is_emitted(&self) -> bool {
        !self.removed
    }
}

/// An `enum` declaration.
#[derive(Clone, Debug)]
pub struct EnumNode {
    /// The enum name.
    pub name: String,
    /// True when the enum was declared with the `flags` keyword.
    pub flags: bool,
    /// The declared underlying type (`byte`, `uint`, ...), if any.
    pub underlying: Option<String>,
    /// The enum members.
    pub members: Vec<Member>,
}

/// A `class` declaration.
#[derive(Clone, Debug)]
pub struct ClassNode {
    /// The class name.
    pub name: String,
    /// True when the class is marked `removed`; such classes are not emitted.
    pub removed: bool,
    /// The `Enum::Member` the class is bound to, if any.
    pub ident: Option<TypeRef>,
    /// The class members.
    pub members: Vec<Member>,
}

/// A top level node.
#[derive(Clone, Debug)]
pub enum Node {
    /// An `enum` declaration.
    Enum(EnumNode),
    /// A `class` declaration.
    Class(ClassNode),
}

impl Node {
    /// The declared name of the node.
    pub fn name(&self) -> &str {
        match self {
            Node::Enum(node) => &node.name,
            Node::Class(node) => &node.name,
        }
    }
}

/// The parsed root of one definition file.
#[derive(Clone, Debug, Default)]
pub struct Root {
    /// The top level nodes in declaration order.
    pub children: Vec<Node>,
}

/// Parser state: a token cursor plus the directory used to resolve imports.
struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
    language_dir: PathBuf,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], language_dir: PathBuf) -> Self {
        Parser {
            tokens,
            position: 0,
            language_dir,
        }
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.position);
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn expect(&mut self, kind: TokenKind) -> LangResult<&'a Token> {
        match self.peek() {
            Some(token) if token.kind == kind => {
                self.position += 1;
                Ok(token)
            }
            Some(token) => Err(error_at(
                token,
                format!(
                    "expecting {} but got '{}'",
                    kind.as_str(),
                    token.value
                ),
            )),
            None => Err(LangError::parse(
                &self
                    .tokens
                    .last()
                    .map(|token| token.file.as_str())
                    .unwrap_or("<input>"),
                self.tokens.last().map(|token| token.line).unwrap_or(1),
                self.tokens.last().map(|token| token.column).unwrap_or(1),
                format!("unexpected end of input, expecting {}", kind.as_str()),
            )),
        }
    }

    fn expect_value(&mut self, kind: TokenKind, value: &str) -> LangResult<&'a Token> {
        match self.peek() {
            Some(token) if token.kind == kind && token.value == value => {
                self.position += 1;
                Ok(token)
            }
            Some(token) => Err(error_at(
                token,
                format!(
                    "expecting {} '{}' but got '{}'",
                    kind.as_str(),
                    value,
                    token.value
                ),
            )),
            None => Err(LangError::parse(
                &self
                    .tokens
                    .last()
                    .map(|token| token.file.as_str())
                    .unwrap_or("<input>"),
                self.tokens.last().map(|token| token.line).unwrap_or(1),
                self.tokens.last().map(|token| token.column).unwrap_or(1),
                format!("unexpected end of input, expecting {} '{}'", kind.as_str(), value),
            )),
        }
    }

    fn optional(&mut self, kind: TokenKind) -> Option<&'a Token> {
        match self.peek() {
            Some(token) if token.kind == kind => {
                self.position += 1;
                Some(token)
            }
            _ => None,
        }
    }

    fn optional_value(&mut self, kind: TokenKind, value: &str) -> Option<&'a Token> {
        match self.peek() {
            Some(token) if token.kind == kind && token.value == value => {
                self.position += 1;
                Some(token)
            }
            _ => None,
        }
    }
}

/// Analyses a token stream and returns the root node.
///
/// `language_dir` is the directory that `#import` paths are resolved against.
pub fn analyze(tokens: &[Token], language_dir: &Path) -> LangResult<Root> {
    let mut parser = Parser::new(tokens, language_dir.to_path_buf());
    let mut root = Root::default();

    while let Some(token) = parser.next() {
        match token.kind {
            TokenKind::Preprocess => {
                let path_token = parser.expect(TokenKind::Str)?.clone();
                if token.value == "import" {
                    let imported = import_file(&path_token.value, &parser.language_dir)?;
                    let tokens = tokenize(&imported.text, &imported.name)?;
                    let nested = analyze(&tokens, &parser.language_dir)?;
                    root.children.extend(nested.children);
                }
            }
            TokenKind::Identifier => match token.value.as_str() {
                "class" => {
                    let node = parse_class(&mut parser, &root)?;
                    root.children.push(Node::Class(node));
                }
                "enum" => {
                    let node = parse_enum(&mut parser, &root)?;
                    root.children.push(Node::Enum(node));
                }
                _ => {}
            },
            _ => {}
        }
    }

    Ok(root)
}

struct ImportedFile {
    name: String,
    text: String,
}

fn import_file(path: &str, language_dir: &Path) -> LangResult<ImportedFile> {
    let direct = language_dir.join(path);
    let candidates = [direct.clone(), PathBuf::from(path)];

    for candidate in candidates {
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).map_err(|source| LangError::Io {
                path: candidate.display().to_string(),
                source,
            })?;
            return Ok(ImportedFile {
                name: candidate.display().to_string(),
                text,
            });
        }
    }

    Err(LangError::Io {
        path: direct.display().to_string(),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "imported definition file not found",
        ),
    })
}

fn parse_enum(parser: &mut Parser<'_>, root: &Root) -> LangResult<EnumNode> {
    let name = parser.expect(TokenKind::Identifier)?.value.clone();

    let mut underlying = None;
    if parser.optional_value(TokenKind::Operator, "<").is_some() {
        let type_name = parser.expect(TokenKind::Identifier)?.value.clone();
        parser.expect_value(TokenKind::Operator, ">")?;
        underlying = Some(type_name);
    }

    let flags = parser.optional_value(TokenKind::Identifier, "flags").is_some();

    let members = parse_inner_scope(parser, root)?;

    Ok(EnumNode {
        name,
        flags,
        underlying,
        members,
    })
}

fn parse_class(parser: &mut Parser<'_>, root: &Root) -> LangResult<ClassNode> {
    let name = parser.expect(TokenKind::Identifier)?.value.clone();

    let mut ident = None;
    if parser.optional_value(TokenKind::Operator, "<").is_some() {
        let target = parser.expect(TokenKind::Identifier)?.value.clone();
        parser.expect_value(TokenKind::Operator, ">")?;
        ident = Some(resolve_full_ident(root, &target, parser)?);
    }

    // `class X expects Parent`: the parent is parsed but never used.
    if parser.optional_value(TokenKind::Identifier, "expects").is_some() {
        parser.expect(TokenKind::Identifier)?;
    }

    let removed = parser.optional_value(TokenKind::Identifier, "removed").is_some();

    let members = parse_inner_scope(parser, root)?;

    Ok(ClassNode {
        name,
        removed,
        ident,
        members,
    })
}

fn parse_inner_scope(parser: &mut Parser<'_>, root: &Root) -> LangResult<Vec<Member>> {
    parser.expect_value(TokenKind::Operator, "{")?;

    let mut members = Vec::new();

    loop {
        if parser.optional_value(TokenKind::Operator, "}").is_some() {
            break;
        }

        let first = parser
            .next()
            .ok_or_else(|| LangError::parse("<input>", 0, 0, "unexpected end of input in scope"))?
            .clone();

        let mut flags_opt = None;
        if parser.optional_value(TokenKind::Operator, "<").is_some() {
            let flag = parser.expect(TokenKind::Identifier)?.value.clone();
            parser.expect_value(TokenKind::Operator, ">")?;
            flags_opt = Some(flag);
        }

        let second = parser.optional(TokenKind::Identifier).cloned();
        let third = parser.optional(TokenKind::Identifier).cloned();

        let (name, type_ref, flags) = match (second, third) {
            (Some(second), Some(third)) => (
                third.value.clone(),
                Some(resolve_weak_ident(root, &second.value)),
                Some(first.value.clone()),
            ),
            (Some(second), None) => (
                second.value.clone(),
                Some(resolve_weak_ident(root, &first.value)),
                None,
            ),
            (None, _) => (first.value.clone(), None, None),
        };

        let mut values = Vec::new();
        if parser.optional_value(TokenKind::Operator, "=").is_some() {
            loop {
                let value = parser
                    .next()
                    .ok_or_else(|| {
                        LangError::parse(&first.file, first.line, first.column, "unexpected end of input in value")
                    })?
                    .clone();
                values.push(value.value.clone());

                if parser.optional_value(TokenKind::Operator, "|").is_some() {
                    continue;
                }
                break;
            }
        }
        parser.expect_value(TokenKind::Terminator, ";")?;

        let mut obsolete = None;
        if parser.optional_value(TokenKind::Identifier, "obsolete").is_some() {
            let mut reason = String::new();
            if let Some(reason_token) = parser.optional(TokenKind::Str) {
                reason = reason_token.value.clone();
            }
            obsolete = Some(reason);
        }

        let mut removed = false;
        if parser.optional_value(TokenKind::Identifier, "removed").is_some() {
            removed = true;
            // The reason is consumed but not part of the generated output.
            if let Some(reason_token) = parser.optional(TokenKind::Str) {
                let _ = reason_token;
            }
        }

        members.push(Member {
            name,
            type_ref,
            flags,
            flags_opt,
            values,
            obsolete,
            removed,
        });
    }

    Ok(members)
}

/// Resolves a plain identifier.
fn resolve_weak_ident(root: &Root, identifier: &str) -> TypeRef {
    if identifier.contains("::") {
        // Handled by `resolve_full_ident`; keep the raw text otherwise.
        return TypeRef::weak(identifier);
    }
    let head = identifier_head(identifier);
    if find_node(root, &head).is_some() {
        TypeRef::local(&head, None)
    } else {
        TypeRef::weak(identifier)
    }
}

/// Resolves an `Enum::Member` identifier.
fn resolve_full_ident(root: &Root, identifier: &str, parser: &Parser<'_>) -> LangResult<TypeRef> {
    let (class, member) = match identifier.split_once("::") {
        Some(parts) => parts,
        None => return Ok(resolve_weak_ident(root, identifier)),
    };

    let class = class.trim_end_matches(':');
    let class_head = identifier_head(class);
    let member_head = identifier_head(member);

    let node = find_node(root, &class_head).ok_or_else(|| {
        let token = parser.peek();
        match token {
            Some(token) => error_at(
                token,
                format!("invalid class in identifier '{}'", identifier),
            ),
            None => LangError::parse("<input>", 0, 0, format!("invalid class in identifier '{}'", identifier)),
        }
    })?;

    let members: Vec<&Member> = match node {
        Node::Enum(node) => node.members.iter().collect(),
        Node::Class(node) => node.members.iter().collect(),
    };

    if !members.iter().any(|member| member.name == member_head) {
        let token = parser.peek();
        let message = format!("invalid property in identifier '{}'", identifier);
        return Err(match token {
            Some(token) => error_at(token, message),
            None => LangError::parse("<input>", 0, 0, message),
        });
    }

    Ok(TypeRef::local(&class_head, Some(member_head)))
}

/// The leading `[a-zA-Z0-9_:]*` part of an identifier; stops at the first
/// character outside that set.
fn identifier_head(identifier: &str) -> String {
    identifier
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':')
        .collect()
}

fn find_node<'a>(root: &'a Root, name: &str) -> Option<&'a Node> {
    root.children.iter().find(|node| node.name() == name)
}

/// Maps every declared type name to the node that declares it.
pub fn type_index(root: &Root) -> HashMap<String, NodeKind> {
    let mut index = HashMap::new();
    for child in &root.children {
        index.insert(child.name().to_owned(), NodeKind::of(child));
    }
    index
}

/// The kind of a top level node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// An `enum` declaration.
    Enum,
    /// A `class` declaration.
    Class,
}

impl NodeKind {
    fn of(node: &Node) -> Self {
        match node {
            Node::Enum(_) => NodeKind::Enum,
            Node::Class(_) => NodeKind::Class,
        }
    }
}
