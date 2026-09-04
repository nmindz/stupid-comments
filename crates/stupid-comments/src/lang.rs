use std::io::Read;
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
    Shell,
    Make,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Self> {
        if let Some(lang) = path.file_name().and_then(|n| n.to_str()).and_then(Self::from_name) {
            return Some(lang);
        }
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
            "sh" | "bash" | "zsh" | "ksh" => Self::Shell,
            "mk" | "make" => Self::Make,
            _ => return None,
        })
    }

    /// Build files and shell rc files carry their type in the name.
    fn from_name(name: &str) -> Option<Self> {
        if name.starts_with("Makefile.") || name.starts_with("makefile.") {
            return Some(Self::Make);
        }
        match name {
            "Makefile" | "makefile" | "GNUmakefile" => Some(Self::Make),
            ".bashrc" | ".bash_profile" | ".bash_aliases" | ".zshrc" | ".zprofile"
            | ".profile" => Some(Self::Shell),
            _ => None,
        }
    }

    /// Same as `from_path`, plus a shebang peek for extensionless files — a
    /// `scripts/` directory is usually full of them, and skipping one silently
    /// is indistinguishable from checking it and finding nothing.
    pub fn from_file(path: &Path) -> Option<Self> {
        if let Some(lang) = Self::from_path(path) {
            return Some(lang);
        }
        if path.extension().is_some() {
            return None;
        }
        let mut buf = [0u8; 128];
        let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
        let head = std::str::from_utf8(&buf[..n]).ok()?;
        let shebang = head.lines().next()?.strip_prefix("#!")?;

        shebang
            .split_whitespace()
            .any(|w| matches!(w.rsplit('/').next(), Some("sh" | "bash" | "zsh" | "ksh" | "dash")))
            .then_some(Self::Shell)
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
            Self::Shell => tree_sitter_bash::LANGUAGE.into(),
            Self::Make => tree_sitter_make::LANGUAGE.into(),
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
            Self::Shell => "shell",
            Self::Make => "make",
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
