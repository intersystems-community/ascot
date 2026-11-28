//! A grammar-debugging binary (`tygr-objectscript <file>`): parses one class or routine with
//! `cos.rs`, then separately parses each method body, since the class grammar captures bodies
//! as raw text and would otherwise hide syntax errors inside them. `tygr-objectscript desugar-sql`
//! prints the core form of each SQL statement on stdin. Not part of the `ascot` CLI.

use std::ops::Deref;
use std::{env, fs, process::exit};
use tygr::{OLC, Parse, Print};
use tygr_objectscript::cos;
use tygr_objectscript::sql::desugar::Desugar;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: tygr-objectscript <file> | desugar-sql");
        exit(1);
    });
    if path == "desugar-sql" {
        return desugar_sql();
    }
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Error reading {path}: {err}");
            exit(1);
        }
    };
    let had_error = if path.ends_with(".cls") {
        check_class(&path, &content)
    } else {
        cos::Routine::parse(&content)
            .map_err(|err| eprintln!("{err}"))
            .is_err()
    };
    if had_error {
        exit(1);
    }
}

/// Each SQL statement on stdin, one per line, in its core form, or `error: ` and the parser's message
/// on one line.
fn desugar_sql() {
    for line in std::io::stdin().lines() {
        let line = line.unwrap_or_else(|err| {
            eprintln!("desugar-sql: {err}");
            exit(1)
        });
        match tygr_objectscript::sql::SQL::parse(&line) {
            Ok(sql) => println!("{}", sql.desugar().print()),
            Err(err) => println!(
                "error: {}",
                err.to_string()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        }
    }
}

// `ClassDefinition::scan` only checks a method's header, not its body -- `Method.body` is
// captured as raw, unparsed text (to allow non-ObjectScript method languages), so a syntax
// error inside one is otherwise invisible until the body is separately parsed here, dispatching
// on `Language`/`CodeMode` the same way `check::mod::MethodBody` does.
fn check_class(path: &str, content: &str) -> bool {
    let Ok(cls) = cos::ClassDefinition::parse(content).map_err(|err| eprintln!("{err}")) else {
        return true;
    };
    let olc = OLC::new(content);
    let mut had_error = false;
    for member in cls.member_list.members.seps.iter() {
        if let cos::Member::Method(method) = member.deref()
            && let Err(err) = check_method_body(method)
        {
            had_error = true;
            // `err.pos()` is relative to the method body's own text, so it's offset by the
            // body's position within the whole file to land on the right line/column.
            let (line, character) = olc
                .offset_to_line_character(method.body.prefixed.wrapped.start + err.pos())
                .unwrap_or_else(|e| e.bound);
            let name = method.header.name.print();
            eprintln!(
                "{path}:{}:{}: in method {name}: {err}",
                line + 1,
                character + 1
            );
        }
    }
    had_error
}

fn check_method_body(method: &cos::Method) -> Result<(), tygr::Error> {
    let (mut language, mut mode) = (None, None);
    for entry in (method.header.keywords.iter()).flat_map(cos::KeywordList::items) {
        match (***entry).as_ref().left() {
            Some(cos::MethodKeyword::Language(v)) => language = Some(&v.value),
            Some(cos::MethodKeyword::CodeMode(v)) => mode = Some(&v.value),
            _ => {}
        }
    }
    let text = method.body.prefixed.wrapped.print();

    let objectscript = matches!(
        language,
        None | Some(cos::MethodLanguage::ObjectScript(..) | cos::MethodLanguage::Cache(..))
    );
    match mode {
        _ if !objectscript => Ok(()),
        None | Some(cos::CodeMode::Code(..)) => cos::ProcedureBody::parse(&text).map(drop),
        Some(cos::CodeMode::Expression(..)) => <cos::ExprBody>::parse(&text).map(drop),
        // Other languages (Python, ...) and modes (Call, Generator, ...) aren't ObjectScript,
        // so there's nothing here to parse.
        _ => Ok(()),
    }
}
