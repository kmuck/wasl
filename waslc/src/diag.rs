//! Diagnostics: stable error codes and display.

use std::fmt;

use crate::source::{SourceMap, Span};

/// Error codes defined by the specification (§ 11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    // Lexical
    InvalidChar,
    InvalidIntLiteral,
    InvalidFloatLiteral,
    InvalidIdent,
    ReservedKeyword,
    InvalidString,
    InvalidCharLiteral,
    // Syntax
    Syntax,
    // Declarations
    Manifest,
    DuplicateItem,
    DuplicateFunc,
    UnknownExportedInterface,
    DuplicateExport,
    DuplicateName,
    DuplicateMember,
    RecursiveExport,
    UnexportedTypeInterface,
    TypeNamedLikeTopLevel,
    EmptyType,
    TooManyFlags,
    UseCycle,
    // Names
    UnknownName,
    UnknownFunc,
    ArgCount,
    ShadowsGlobal,
    DuplicateLocal,
    UnknownType,
    UnknownField,
    UnknownCase,
    // Types
    TypeMismatch,
    LiteralOutOfRange,
    UntypedLiteral,
    InvalidOperator,
    InvalidConversion,
    NonBoolCondition,
    RecordFields,
    Payload,
    CannotInfer,
    NonConstGlobal,
    Uninitialized,
    NotMatchable,
    FlagsOrder,
    // Control flow
    AssignImmutable,
    BreakOutsideLoop,
    MissingReturnValue,
    UnexpectedReturnValue,
    MissingReturn,
    AssignReadOnly,
    NonExhaustive,
    RedundantCase,
    // Input/output and internal errors (outside the specification)
    Io,
    UnknownWorld,
    Internal,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::InvalidChar => "E0001",
            Code::InvalidIntLiteral => "E0002",
            Code::InvalidFloatLiteral => "E0003",
            Code::InvalidIdent => "E0004",
            Code::ReservedKeyword => "E0005",
            Code::InvalidString => "E0006",
            Code::InvalidCharLiteral => "E0007",
            Code::Syntax => "E0100",
            Code::Manifest => "E0200",
            Code::DuplicateItem => "E0201",
            Code::DuplicateFunc => "E0202",
            Code::UnknownExportedInterface => "E0203",
            Code::DuplicateExport => "E0204",
            Code::DuplicateName => "E0206",
            Code::DuplicateMember => "E0207",
            Code::RecursiveExport => "E0208",
            Code::UnexportedTypeInterface => "E0209",
            Code::TypeNamedLikeTopLevel => "E0210",
            Code::EmptyType => "E0211",
            Code::TooManyFlags => "E0212",
            Code::UseCycle => "E0213",
            Code::UnknownName => "E0300",
            Code::UnknownFunc => "E0301",
            Code::ArgCount => "E0302",
            Code::ShadowsGlobal => "E0303",
            Code::DuplicateLocal => "E0304",
            Code::UnknownType => "E0305",
            Code::UnknownField => "E0306",
            Code::UnknownCase => "E0307",
            Code::TypeMismatch => "E0400",
            Code::LiteralOutOfRange => "E0401",
            Code::UntypedLiteral => "E0402",
            Code::InvalidOperator => "E0403",
            Code::InvalidConversion => "E0404",
            Code::NonBoolCondition => "E0405",
            Code::RecordFields => "E0406",
            Code::Payload => "E0407",
            Code::CannotInfer => "E0408",
            Code::NonConstGlobal => "E0409",
            Code::Uninitialized => "E0410",
            Code::NotMatchable => "E0411",
            Code::FlagsOrder => "E0412",
            Code::AssignImmutable => "E0500",
            Code::BreakOutsideLoop => "E0501",
            Code::MissingReturnValue => "E0502",
            Code::UnexpectedReturnValue => "E0503",
            Code::MissingReturn => "E0504",
            Code::AssignReadOnly => "E0505",
            Code::NonExhaustive => "E0506",
            Code::RedundantCase => "E0507",
            Code::Io => "E9000",
            Code::UnknownWorld => "E9001",
            Code::Internal => "E9999",
        }
    }
}

/// A compilation error.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: Code,
    pub message: String,
    /// Location of the error; absent for errors not tied to a source file.
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn new(code: Code, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            message: message.into(),
            span: Some(span),
        }
    }

    pub fn without_span(code: Code, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            message: message.into(),
            span: None,
        }
    }

    /// Displays the diagnostic as `<file>:<line>:<column>: error[<code>]: <message>`.
    pub fn display<'a>(&'a self, sources: &'a SourceMap) -> impl fmt::Display + 'a {
        DisplayDiagnostic {
            diag: self,
            sources,
        }
    }
}

struct DisplayDiagnostic<'a> {
    diag: &'a Diagnostic,
    sources: &'a SourceMap,
}

impl fmt::Display for DisplayDiagnostic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.diag.span {
            let file = self.sources.get(span.file);
            let (line, col) = file.line_col(span.start);
            write!(f, "{}:{}:{}: ", file.path.display(), line, col)?;
        }
        write!(
            f,
            "error[{}]: {}",
            self.diag.code.as_str(),
            self.diag.message
        )
    }
}

pub type Result<T> = std::result::Result<T, Diagnostic>;

/// Shorthand: returns an error at the given location.
pub fn err<T>(code: Code, span: Span, message: impl Into<String>) -> Result<T> {
    Err(Diagnostic::new(code, span, message))
}
