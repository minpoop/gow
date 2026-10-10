use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColType {
    Id,
    Str,
    Text,
    Int,
    Float,
    Bool,
    Color,
    Enum,
    Ref,
    RefOpt,
    ListRef,
    ListEnum,
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub ty: ColType,
    /// Allowed values for `Enum` / `ListEnum`.
    pub values: Vec<String>,
    /// Target sheet for `Ref` / `RefOpt` / `ListRef`.
    pub sheet: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Shared Rust enum name (several columns may share one value set).
    pub enum_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub struct_name: String,
    pub about: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Map<String, Value>>,
}

impl Sheet {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }
    pub fn row_id(row: &Map<String, Value>) -> Option<&str> {
        row.get("id").and_then(Value::as_str)
    }
    pub fn find(&self, id: &str) -> Option<&Map<String, Value>> {
        self.rows.iter().find(|r| Self::row_id(r) == Some(id))
    }
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.rows.iter().filter_map(|r| Self::row_id(r))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Sheets {
    pub sheets: Vec<Sheet>,
}

#[derive(Deserialize)]
struct RawColumn {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    values: Vec<String>,
    sheet: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawSheet {
    sheet: String,
    #[serde(rename = "struct")]
    struct_name: String,
    #[serde(default)]
    about: String,
    columns: indexmap::IndexMap<String, RawColumn>,
    rows: Vec<Map<String, Value>>,
}

fn column_from_raw(name: &str, raw: RawColumn) -> Result<Column> {
    let (ty, needs_values, needs_sheet) = match raw.ty.as_str() {
        "id" => (ColType::Id, false, false),
        "string" => (ColType::Str, false, false),
        "text" => (ColType::Text, false, false),
        "int" => (ColType::Int, false, false),
        "float" => (ColType::Float, false, false),
        "bool" => (ColType::Bool, false, false),
        "color" => (ColType::Color, false, false),
        "enum" => (ColType::Enum, true, false),
        "ref" => (ColType::Ref, false, true),
        "ref?" => (ColType::RefOpt, false, true),
        "list:ref" => (ColType::ListRef, false, true),
        "list:enum" => (ColType::ListEnum, true, false),
        other => bail!("column `{name}`: unknown type `{other}`"),
    };
    if needs_values && raw.values.is_empty() {
        bail!("column `{name}`: enum without values");
    }
    if needs_sheet && raw.sheet.is_none() {
        bail!("column `{name}`: ref without target sheet");
    }
    Ok(Column {
        name: name.to_string(),
        ty,
        values: raw.values,
        sheet: raw.sheet,
        min: raw.min,
        max: raw.max,
        enum_name: raw.name,
    })
}

impl Sheets {
    /// Load every `*.json` sheet in `dir`, sorted by sheet name.
    pub fn load(dir: &Path) -> Result<Self> {
        let mut sheets = Vec::new();
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        paths.sort();
        for p in paths {
            let text = std::fs::read_to_string(&p).with_context(|| p.display().to_string())?;
            let raw: RawSheet =
                serde_json::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
            let mut columns = Vec::new();
            for (name, rc) in raw.columns {
                columns.push(column_from_raw(&name, rc).map_err(|e| anyhow!("{}: {e}", p.display()))?);
            }
            sheets.push(Sheet {
                name: raw.sheet,
                struct_name: raw.struct_name,
                about: raw.about,
                columns,
                rows: raw.rows,
            });
        }
        sheets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Sheets { sheets })
    }

    pub fn get(&self, name: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.name == name)
    }
}
