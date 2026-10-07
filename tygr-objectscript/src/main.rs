//! A grammar-debugging binary (`tygr-objectscript <file>`): parses one class or routine with
//! `cos.rs`, then separately parses each method body, since the class grammar captures bodies
//! as raw text and would otherwise hide syntax errors inside them. Not part of the `ascot` CLI.

use std::ops::Deref;
use std::{env, fs, process::exit};
use tygr::{Grammar, OLC};
use tygr_objectscript::cos;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: tygr-objectscript <file>");
        exit(1);
    });
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
        match cos::Routine::scan(&content) {
            Ok(()) => false,
            Err(err) => {
                eprintln!("{err}");
                true
            }
        }
    };
    if had_error {
        exit(1);
    }
}

// `ClassDefinition::scan` only checks a method's header, not its body -- `Method.body` is
// captured as raw, unparsed text (to allow non-ObjectScript method languages), so a syntax
// error inside one is otherwise invisible until the body is separately parsed here, dispatching
// on `Language`/`CodeMode` the same way `check::mod::MethodBody` does.
fn check_class(path: &str, content: &str) -> bool {
    let cls = match cos::ClassDefinition::parse(content) {
        Ok(cls) => cls,
        Err(err) => {
            eprintln!("{err}");
            return true;
        }
    };
    let olc = OLC::new(content);
    let mut had_error = false;
    for member in cls.member_list.members.seps.iter() {
        if let cos::Member::Method(method) = member.deref()
            && let Err((body_start, err)) = check_method_body(method)
        {
            had_error = true;
            report(path, &olc, &method.header.name.print(), body_start, &err);
        }
    }
    had_error
}

fn check_method_body(method: &cos::Method) -> Result<(), (usize, tygr::Error)> {
    let keyword = |name| {
        method.header.keywords.as_ref().and_then(|keywords| {
            let cos::KeywordEntry::Assign { value, .. } = keywords.get(name)? else {
                return None;
            };
            Some(value.print())
        })
    };
    let language = keyword("Language").and_then(|v| cos::MethodLanguage::parse(&v).ok());
    let mode = keyword("CodeMode").and_then(|v| cos::CodeMode::parse(&v).ok());
    let body = &method.body.prefixed.wrapped;
    let text = body.print();

    match (&language, &mode) {
        (
            None | Some(cos::MethodLanguage::ObjectScript(..)),
            None | Some(cos::CodeMode::Code(..)),
        ) => cos::ProcedureBody::scan(&text),
        (
            None | Some(cos::MethodLanguage::ObjectScript(..)),
            Some(cos::CodeMode::Expression(..)),
        ) => cos::ExprEntry::scan(&text),
        // Other languages (Python, ...) and modes (Call, Generator, ...) aren't ObjectScript,
        // so there's nothing here to parse.
        _ => Ok(()),
    }
    .map_err(|err| (body.start, err))
}

// `err.pos()` is relative to the method body's own text, so it's offset by `body_start` (the
// body's position within the whole file) to land on the right line/column.
fn report(path: &str, olc: &OLC, method_name: &str, body_start: usize, err: &tygr::Error) {
    let (line, character) = olc.offset_to_line_character(body_start + err.pos());
    eprintln!(
        "{path}:{}:{}: in method {method_name}: {err}",
        line + 1,
        character + 1
    );
}
