//! A declaration's `[ ... ]` keywords as the checker and the bindings see them: the grammar's
//! reading of each entry (`cos::ClassKeyword`) folded into one struct per kind of declaration,
//! and what didn't fit, for `check_cls` to report.

use tygr::Grammar;

use crate::bindings::{
    ClassType, ClientDataType, Inheritance, Language, OdbcType, SoapBindingStyle, SoapBodyUse,
    SqlCategory,
};
use crate::cos::{self, ClassKeyword, KeywordIssue, KeywordList, Typed};
use crate::details::{Positions, WithRange};

// An entry that isn't the keyword it names, by the name as written.
pub struct Issue {
    pub keyword: String,
    pub why: KeywordIssue,
}

// The grammar's spellings to the host's enums, variant for variant.
macro_rules! same_variants {
    ($($t:ident { $($v:ident),* $(,)? })*) => {$(
        impl From<&cos::$t> for $t {
            fn from(value: &cos::$t) -> Self {
                match value { $(cos::$t::$v(_) => Self::$v,)* }
            }
        }
    )*};
}

same_variants! {
    ClassType { Datatype, Dynamic, Index, Literal, Persistent, Serial, Stream, View }
    ClientDataType {
        Bigint, BinaryStream, Binary, Boolean, CharacterStream, Currency, Date, Decimal, Double,
        Fdate, Ftimestamp, Handle, Integer, List, LongVarchar, Numeric, Status, Timestamp, Time,
        Varchar,
    }
    OdbcType {
        Bigint, Bit, Date, Double, Integer, LongVarbinary, LongVarchar, Numeric, ResultSet,
        Smallint, Struct, Timestamp, Time, Tinyint, Varbinary, Varchar,
    }
    SqlCategory {
        Date, Double, Fmdate, Fmtimestamp, Integer, Mvdate, Name, Numeric, String, Timestamp, Time,
    }
    Inheritance { Left, Right }
    SoapBindingStyle { Document, Rpc }
    SoapBodyUse { Literal, Encoded }
}

impl From<&cos::MethodLanguage> for Language {
    fn from(language: &cos::MethodLanguage) -> Self {
        match language {
            cos::MethodLanguage::ObjectScript(_) => Self::Objectscript,
            cos::MethodLanguage::Python(_) => Self::Python,
            cos::MethodLanguage::TSQL(_) => Self::Tsql,
            cos::MethodLanguage::ISPL(_) => Self::Ispl,
            cos::MethodLanguage::JavaScript(_) => Self::Javascript,
        }
    }
}

fn names(list: &cos::ClassList) -> Vec<String> {
    list.classes().iter().map(|class| class.print()).collect()
}

// A class's keywords, as declared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassKeywords {
    pub abstract_: bool,
    pub class_type: Option<ClassType>,
    pub client_data_type: Option<ClientDataType>,
    pub client_name: Option<String>,
    pub compile_after: Vec<String>,
    pub ddl_allowed: bool,
    pub depends_on: Vec<String>,
    pub deprecated: bool,
    pub final_: bool,
    pub generated_by: Option<String>,
    pub hidden: bool,
    pub inheritance: Option<Inheritance>,
    pub language: Option<Language>,
    pub legacy_instance_context: bool,
    pub no_extent: bool,
    pub odbc_type: Option<OdbcType>,
    pub owner: Option<String>,
    pub procedure_block: Option<bool>,
    pub property_class: Vec<String>,
    pub server_only: bool,
    pub sharded: bool,
    pub soap_binding_style: Option<SoapBindingStyle>,
    pub soap_body_use: Option<SoapBodyUse>,
    pub sql_category: Option<SqlCategory>,
    pub sql_row_id_name: Option<String>,
    pub sql_row_id_private: bool,
    pub sql_table_name: Option<String>,
    pub storage_strategy: Option<String>,
    pub system: Option<u32>,
    pub view_query: Option<String>,
}

impl ClassKeywords {
    // The class's keywords, and every entry that isn't one.
    pub fn read(
        list: &Option<KeywordList<Typed<ClassKeyword>>>,
        pos: &impl Positions,
    ) -> (Self, Vec<WithRange<Issue>>) {
        let mut k = Self::default();
        let mut issues = Vec::new();
        for entry in list.iter().flat_map(KeywordList::items) {
            let Typed { entry, keyword } = &***entry;
            let keyword = match keyword {
                Ok(keyword) => keyword,
                Err(why) => {
                    let name = entry.keyword();
                    let issue = Issue {
                        keyword: name.print(),
                        why: why.clone(),
                    };
                    issues.push(WithRange::new(issue, pos.range(&name.0)));
                    continue;
                }
            };
            match keyword {
                ClassKeyword::Abstract(on) => k.abstract_ = *on,
                ClassKeyword::ClassType(t) => k.class_type = Some(t.into()),
                ClassKeyword::ClientDataType(t) => k.client_data_type = Some(t.into()),
                ClassKeyword::ClientName(v) => k.client_name = Some(v.print()),
                ClassKeyword::CompileAfter(l) => k.compile_after = names(l),
                ClassKeyword::DdlAllowed(on) => k.ddl_allowed = *on,
                ClassKeyword::DependsOn(l) => k.depends_on = names(l),
                ClassKeyword::Deprecated(on) => k.deprecated = *on,
                ClassKeyword::Final(on) => k.final_ = *on,
                ClassKeyword::GeneratedBy(v) => k.generated_by = Some(v.print()),
                ClassKeyword::Hidden(on) => k.hidden = *on,
                ClassKeyword::Inheritance(i) => k.inheritance = Some(i.into()),
                ClassKeyword::Language(l) => k.language = Some(l.into()),
                ClassKeyword::LegacyInstanceContext(on) => k.legacy_instance_context = *on,
                ClassKeyword::NoExtent(on) => k.no_extent = *on,
                ClassKeyword::OdbcType(t) => k.odbc_type = Some(t.into()),
                ClassKeyword::Owner(v) => k.owner = Some(v.print()),
                ClassKeyword::ProcedureBlock(on) => k.procedure_block = Some(*on),
                ClassKeyword::PropertyClass(l) => k.property_class = names(l),
                ClassKeyword::ServerOnly(on) => k.server_only = *on,
                ClassKeyword::Sharded(on) => k.sharded = *on,
                ClassKeyword::SoapBindingStyle(s) => k.soap_binding_style = Some(s.into()),
                ClassKeyword::SoapBodyUse(u) => k.soap_body_use = Some(u.into()),
                ClassKeyword::SqlCategory(c) => k.sql_category = Some(c.into()),
                ClassKeyword::SqlRowIdName(v) => k.sql_row_id_name = Some(v.print()),
                ClassKeyword::SqlRowIdPrivate(on) => k.sql_row_id_private = *on,
                ClassKeyword::SqlTableName(v) => k.sql_table_name = Some(v.print()),
                ClassKeyword::StorageStrategy(v) => k.storage_strategy = Some(v.print()),
                ClassKeyword::System(d) => k.system = d.print().parse().ok(),
                ClassKeyword::ViewQuery(v) => k.view_query = Some(v.print()),
            }
        }
        (k, issues)
    }
}

// A `(a, b)` or single-item list value.
pub fn class_list_text(value: &str) -> Vec<String> {
    let value = value.trim();
    let value = value
        .strip_prefix('(')
        .and_then(|v| v.strip_suffix(')'))
        .unwrap_or(value);
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
