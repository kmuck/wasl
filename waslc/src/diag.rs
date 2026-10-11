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
    TooManyParams,
    // Names
    UnknownName,
    UnknownFunc,
    ArgCount,
    ShadowsGlobal,
    DuplicateLocal,
    // Types
    TypeMismatch,
    LiteralOutOfRange,
    UntypedLiteral,
    InvalidOperator,
    InvalidConversion,
    NonBoolCondition,
    NonConstGlobal,
    // Control flow
    AssignImmutable,
    BreakOutsideLoop,
    MissingReturnValue,
    UnexpectedReturnValue,
    MissingReturn,
    // Input/output and internal errors (outside the specification)
    Io,
    UnknownWorld,
    /// Construct of the v0.2 specification that the compiler does not implement yet.
    Unsupported,
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
            Code::TooManyParams => "E0205",
            Code::UnknownName => "E0300",
            Code::UnknownFunc => "E0301",
            Code::ArgCount => "E0302",
            Code::ShadowsGlobal => "E0303",
            Code::DuplicateLocal => "E0304",
            Code::TypeMismatch => "E0400",
            Code::LiteralOutOfRange => "E0401",
            Code::UntypedLiteral => "E0402",
            Code::InvalidOperator => "E0403",
            Code::InvalidConversion => "E0404",
            Code::NonBoolCondition => "E0405",
            Code::NonConstGlobal => "E0409",
            Code::AssignImmutable => "E0500",
            Code::BreakOutsideLoop => "E0501",
            Code::MissingReturnValue => "E0502",
            Code::UnexpectedReturnValue => "E0503",
            Code::MissingReturn => "E0504",
            Code::Io => "E9000",
            Code::UnknownWorld => "E9001",
            Code::Unsupported => "E9002",
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
