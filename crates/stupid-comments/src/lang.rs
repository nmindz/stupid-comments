use std::path::Path;
use tree_sitter::Language;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    Go,
    Kotlin,
    Json,
    Toml,
    Yaml,
    Hcl,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "rs" => Self::Rust,
            "go" => Self::Go,
            "kt" | "kts" => Self::Kotlin,
            "json" | "jsonc" | "json5" => Self::Json,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "tf" | "tfvars" | "hcl" => Self::Hcl,
            _ => return None,
        })
    }

    pub fn language(self) -> Language {
        match self {
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
            Self::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Self::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            Self::Hcl => tree_sitter_hcl::LANGUAGE.into(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Kotlin => "kotlin",
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Hcl => "hcl",
        }
    }

    /// Config formats whose comments are whole `#` lines, which a line scan
    /// recovers when templating (Helm, and friends) defeats the grammar.
    pub fn hash_line_comments(self) -> bool {
        matches!(self, Self::Yaml | Self::Toml | Self::Hcl)
    }

    /// Grammar maturity is uneven here; findings stay non-blocking.
    pub fn is_provisional(self) -> bool {
        matches!(self, Self::Kotlin)
    }

    pub fn doc_prefixes(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["///", "//!", "/**"],
            Self::Go | Self::Kotlin => &["/**"],
            Self::JavaScript | Self::TypeScript | Self::Tsx => &["/**"],
            _ => &[],
        }
    }
}
