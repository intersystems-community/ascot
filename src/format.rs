//! The shape pass: one walk over a document's command lines, shared by the style checker and the
//! formatter, apart from the type check. Each finding is a style rule a spot breaks and, when one
//! edit satisfies the rule, that edit. The checker reports every finding (the `S` codes); the
//! formatter applies the edits inside procedure blocks and method bodies, whose lines no
//! `Label+offset` reaches (IRIS raises `<NOLINE>`), so a line may be re-divided there, and leaves
//! the rest of a routine as written. An offset counted from a label before a block to a line after
//! it passes through the block, though: a routine that reaches a line by offset anywhere keeps its
//! line count, and only same-line edits apply in it. Function keywords and the commas inside
//! parentheses stay the type check's to report: they sit in expressions, which only it walks.

use std::ops::Deref;

use tygr::{Grammar, OLC, Range};
use tygr_objectscript::cos::{
    self, Block, Command, CommandLike, Conjunct, CurryBlock, GenericExpr, InfixOperation,
    KeywordKind, Operator, PostCondition, ProcedureBody, RoutineMain,
};

use crate::{
    DocKind,
    bindings::{AndOperator, Diagnostic, Language},
    check::{Break, TypeError, TypeErrorKind},
    details::{ClassDetails, MemberKindSpecificDetails, Positions},
    style::Styles,
};

type Span = std::ops::Range<usize>;

/// A style rule broken at `range`, with the one edit that satisfies it, when one does.
pub struct Finding {
    pub kind: TypeErrorKind,
    pub range: Span,
    pub edit: Option<Edit>,
}

/// `text` in place of the source from `start` to `end`, byte offsets.
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Finding {
    pub(crate) fn diagnostic(self, olc: &OLC) -> Diagnostic {
        let (kind, range) = (self.kind, olc.span(self.range.start, self.range.end));
        let related_information = vec![];
        TypeError {
            kind,
            range,
            related_information,
        }
        .into()
    }
}

/// The findings of a routine, parsed as written (its macros unexpanded). `indent` is one level of
/// indentation for a line the formatter adds; `None` reports without editing.
pub(crate) fn findings_rtn(
    rtn: &cos::Routine,
    text: &str,
    style: &Styles,
    indent: Option<&str>,
) -> Vec<Finding> {
    let mut pass = Pass::new(text, 0, style, indent);
    pass.routine_main(&rtn.main, Ctx::new(false));
    pass.elements(
        rtn.subroutines.iter().flat_map(|subs| subs.items.iter()),
        Ctx::new(false),
    );
    if pass.offsets {
        // The edits that add or remove a line: a gap's, never a keyword's.
        for finding in &mut pass.findings {
            let reshapes = |e: &Edit| e.text.contains('\n') || text[e.start..e.end].contains('\n');
            finding.edit = finding.edit.take().filter(|e| !reshapes(e));
        }
    }
    pass.findings
}

/// The findings of a class's ObjectScript method bodies. A method that is no procedure block is
/// plain code to IRIS, which an offset reaches: reported, never edited.
pub(crate) fn findings_cls(
    cls: &ClassDetails,
    text: &str,
    olc: &OLC,
    style: &Styles,
    indent: Option<&str>,
) -> Vec<Finding> {
    let mut findings = vec![];
    for member in cls.members.in_order() {
        let MemberKindSpecificDetails::MethodOrClassMethod(method) = &member.it.kind else {
            continue;
        };
        let language = method.language.or(cls.keywords.language);
        if !matches!(language, None | Some(Language::Objectscript))
            || !matches!(method.mode, None | Some(cos::CodeMode::Code(..)))
        {
            continue;
        }
        let Ok(body) = <ProcedureBody>::parse(&method.body.it) else {
            continue;
        };
        let base = olc.offset(method.body.range.start);
        let mut pass = Pass::new(text, base, style, indent);
        pass.procedure_body(&body, Ctx::new(method.procedure_block.is_some()));
        findings.extend(pass.findings);
    }
    findings
}

/// The edits that bring the procedure blocks and method bodies of `text`, the document at `uri`,
/// to `style`: in order and disjoint. `indent` is one level of indentation, for a line an edit
/// adds. `None` when the document doesn't parse.
pub fn edits(text: &str, uri: &str, style: &Styles, indent: &str) -> Option<Vec<Edit>> {
    // A line at column 0 is a label: an added line is always indented.
    let indent = Some(indent).filter(|i| !i.is_empty()).unwrap_or("\t");
    let findings = match DocKind::from_uri(uri)? {
        DocKind::Cls => {
            let cls = ClassDetails::try_from(text).ok()?;
            findings_cls(&cls, text, &OLC::new(text), style, Some(indent))
        }
        DocKind::Rtn => {
            let rtn = cos::Routine::parse(text).ok()?;
            findings_rtn(&rtn, text, style, Some(indent))
        }
        DocKind::Inc => vec![],
    };
    let mut edits: Vec<Edit> = findings.into_iter().filter_map(|f| f.edit).collect();
    edits.sort_by_key(|edit| (edit.start, edit.end));
    edits.dedup_by(|next, kept| next.start < kept.end);
    Some(edits)
}

/// `text` with `edits`, as `edits` returns them, applied.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for edit in edits {
        out.push_str(&text[at..edit.start]);
        out.push_str(&edit.text);
        at = edit.end;
    }
    out.push_str(&text[at..]);
    out
}

struct Pass<'a> {
    text: &'a str,
    // Where the parsed text starts in `text`: a method body's offset into its class, 0 for a routine.
    base: usize,
    style: &'a Styles,
    indent: Option<&'a str>,
    newline: &'static str,
    findings: Vec<Finding>,
    // Whether the text reaches a line by `Label+offset`, which keeps every line where it is.
    offsets: bool,
}

#[derive(Clone, Copy)]
struct Ctx {
    // A procedure block or method body, which the formatter may edit.
    block: bool,
    // A loop or a `Try`/`Catch` block, which `Quit` leaves instead of the body.
    in_loop: bool,
    // Where the command owning the current block starts: its line's indentation is the block's.
    anchor: usize,
}

impl Ctx {
    fn new(block: bool) -> Self {
        Ctx {
            block,
            in_loop: false,
            anchor: 0,
        }
    }
}

impl<'a> Pass<'a> {
    fn new(text: &'a str, base: usize, style: &'a Styles, indent: Option<&'a str>) -> Self {
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let (findings, offsets) = (vec![], false);
        Pass {
            text,
            base,
            style,
            indent,
            newline,
            findings,
            offsets,
        }
    }

    fn report(&mut self, kind: TypeErrorKind, at: Span, edit: Option<Edit>) {
        let base = self.base;
        let edit = edit.map(|e| Edit {
            start: base + e.start,
            end: base + e.end,
            text: e.text,
        });
        let range = base + at.start..base + at.end;
        self.findings.push(Finding { kind, range, edit });
    }

    // The formatter's edit, where it formats: a procedure block or method body.
    fn edit(&self, ctx: Ctx, span: Span, text: String) -> Option<Edit> {
        let (start, end) = (span.start, span.end);
        (ctx.block && self.indent.is_some()).then_some(Edit { start, end, text })
    }

    fn slice(&self, span: Span) -> &'a str {
        &self.text[self.base + span.start..self.base + span.end]
    }

    // The leading whitespace of the line `offset` is on; one level where there is none, since a
    // line at column 0 would be a label.
    fn line_indent(&self, offset: usize) -> &'a str {
        let line = self.text[..self.base + offset]
            .rfind('\n')
            .map_or(0, |nl| nl + 1);
        let rest = &self.text[line..];
        let indent = &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()];
        if indent.is_empty() {
            self.indent.unwrap_or("\t")
        } else {
            indent
        }
    }

    fn routine_main(&mut self, main: &RoutineMain, ctx: Ctx) {
        let lines = || (main.line_1.iter()).chain(main.line_n.iter().flat_map(|l| l.items.iter()));
        self.new_per_block(lines().flat_map(commands_of));
        for line in lines() {
            self.command_line(line, ctx);
        }
    }

    fn procedure_body(&mut self, body: &ProcedureBody, ctx: Ctx) {
        self.new_per_block(body_commands(&body.main).map(|(command, _)| command));
        if let Some(line) = &body.main.line_1 {
            self.command_line(line, ctx);
        }
        for line in body.main.line_n.iter().flat_map(|l| l.items.iter()) {
            self.command_line(line, ctx);
        }
        self.elements(
            body.subroutines.iter().flat_map(|subs| subs.items.iter()),
            ctx,
        );
    }

    // The labels after a body's main lines: a plain one shares the body's region, a procedure
    // block has one of its own, and the lines after its `}` are the body's again.
    fn elements<'e>(&mut self, elements: impl Iterator<Item = &'e cos::RoutineElement>, ctx: Ctx) {
        let ctx = Ctx {
            in_loop: false,
            ..ctx
        };
        for element in elements {
            match element {
                cos::RoutineElement::Procedure(procedure) => {
                    self.procedure_body(&procedure.body.wrapped, Ctx { block: true, ..ctx });
                    if let Some(trailer) = &procedure.trailer {
                        self.routine_main(&trailer.lines, ctx);
                    }
                }
                cos::RoutineElement::Subroutine(label) => self.routine_main(&label.body, ctx),
            }
        }
    }

    fn new_per_block<'c>(&mut self, commands: impl Iterator<Item = &'c Range<Command>>) {
        let Some(max) = self.style.rules.max_new_per_block else {
            return;
        };
        let news = commands.filter(|c| matches!(***c, Command::New(_)));
        for command in news.skip(max as usize) {
            let at = command.start..command.end;
            self.report(TypeErrorKind::NewPerBlock { max }, at, None);
        }
    }

    fn command_line<S>(&mut self, line: &cos::CommandLine<S>, ctx: Ctx) {
        // A dotted line is the block of an argumentless `Do`, which isn't modeled: skipped.
        if !line.periods.is_empty() {
            return;
        }
        if let Some(max) = self.style.rules.max_commands_per_line {
            for command in commands_of(line).skip(max as usize) {
                let at = command.start..command.end;
                self.report(TypeErrorKind::CommandsPerLine { max }, at, None);
            }
        }
        for command in commands_of(line) {
            self.command(command, ctx);
        }
    }

    fn command(&mut self, command: &Range<Command>, ctx: Ctx) {
        for kw in command.keywords(command.start) {
            let Some(expected) = self.style.keywords.expected(&kw) else {
                continue;
            };
            let at = kw.start..kw.start + kw.text.len();
            let edit = self.edit(ctx, at.clone(), expected.clone());
            let sigil = if kw.kind == KeywordKind::Function {
                "$"
            } else {
                ""
            };
            let (found, expected) = (format!("{sigil}{}", kw.text), format!("{sigil}{expected}"));
            let kind = TypeErrorKind::KeywordStyle {
                kind: kw.kind,
                found,
                expected,
            };
            self.report(kind, at, edit);
        }
        let ctx = Ctx {
            anchor: command.start,
            ..ctx
        };
        let in_loop = Ctx {
            in_loop: true,
            ..ctx
        };
        match command.deref() {
            Command::If(cos::IfCommand { cnds, branches, .. }) => {
                if let Some(cnds) = cnds {
                    self.conditions(&cnds.first, &cnds.remaining);
                }
                let Some(branches) = branches else {
                    return;
                };
                self.block(&branches.thn, ctx);
                let mut close = close_brace_of(&branches.thn);
                for eif in &branches.eifs {
                    self.chain(close, &eif.kw, "ElseIf", ctx);
                    self.conditions(&eif.cnds.first, &eif.cnds.remaining);
                    self.block(&eif.thn, ctx);
                    close = close_brace_of(&eif.thn);
                }
                if let Some(els) = &branches.els {
                    self.chain(close, &els.kw, "Else", ctx);
                    if let Some(body) = &els.body {
                        self.block(body, ctx);
                    }
                }
            }
            Command::ElseIf(branch) => {
                self.conditions(&branch.cnds.first, &branch.cnds.remaining);
                self.block(&branch.thn, ctx);
            }
            Command::Else(cos::ElseBranch {
                body: Some(body), ..
            }) => self.block(body, ctx),
            Command::While(cos::WhileCommand { cnds, bdy, .. }) => {
                if let Some(cnds) = cnds {
                    self.conditions(&cnds.first, &cnds.remaining);
                }
                self.curry(bdy, in_loop);
            }
            Command::DoWhile(cos::DoWhileCommand { iter, cnds, .. }) => {
                self.curry(iter, in_loop);
                if let Some(cnds) = cnds {
                    self.conditions(&cnds.first, &cnds.remaining);
                }
            }
            Command::For(cos::ForCommand { bdy: Some(bdy), .. }) => self.block(bdy, in_loop),
            Command::Try(cos::TryCommand {
                b1, kw_catch, b2, ..
            }) => {
                self.block(b1, in_loop);
                self.chain(close_brace_of(b1), kw_catch, "Catch", ctx);
                self.block(b2, in_loop);
            }
            Command::Do(cos::DoCommand { args, .. }) => {
                for arg in args.iter().flat_map(|args| args.items.iter()) {
                    self.argument_postconditional("do", &arg.pc);
                    if let cos::DoTarget::Plain(entry) = &arg.entry_ref
                        && let cos::EntryRefAndArgs::Legacy { proc, .. } = entry.deref()
                    {
                        self.offsets |= has_offset(proc);
                    }
                }
            }
            Command::Goto(cos::GotoCommand { args, .. }) => {
                for arg in args.iter().flat_map(|args| args.items.iter()) {
                    self.argument_postconditional("goto", &arg.pc);
                    if let cos::AllowCase::Plain(cos::Location(cos::AllowIndirection::Plain(e))) =
                        &arg.location
                    {
                        self.offsets |= has_offset(e);
                    }
                }
            }
            Command::Job(cos::JobCommand { args, .. }) => {
                for arg in &args.items {
                    if let cos::JobArgument::RoutineLocal(entry, ..)
                    | cos::JobArgument::RoutineWithLocation(entry, ..) = arg
                    {
                        self.offsets |= has_offset(entry);
                    }
                }
            }
            Command::Xecute(cos::XecuteCommand { args, .. }) => {
                for arg in &args.items {
                    self.argument_postconditional("xecute", &arg.pc);
                }
            }
            Command::Quit(_)
                if self.style.rules.allow_quit_for_return == Some(false) && !ctx.in_loop =>
            {
                self.report(TypeErrorKind::QuitExits, command.start..command.end, None);
            }
            _ => {}
        }
    }

    // The commas, or the `&&`s, the style doesn't want between an `If`'s or `While`'s conditions.
    fn conditions<B: Grammar>(&mut self, first: &GenericExpr<B>, remaining: &[Conjunct]) {
        let Some(wanted) = self.style.rules.condition_and else {
            return;
        };
        let kind = || TypeErrorKind::AndOperator { wanted };
        match wanted {
            AndOperator::DoubleAmpersand => {
                for cnd in remaining {
                    self.report(kind(), cnd.expr.start..cnd.expr.end, None);
                }
            }
            AndOperator::Comma => {
                let ops = (first.ops.iter()).chain(remaining.iter().flat_map(|cnd| cnd.ops.iter()));
                for op in ops {
                    if let InfixOperation::Other { op: operator, .. } = op.deref()
                        && matches!(operator.deref(), Operator::LA(_))
                    {
                        self.report(kind(), op.start..op.end, None);
                    }
                }
            }
        }
    }

    // `command` is the full name, lowercased, as the style keys it.
    fn argument_postconditional(&mut self, command: &str, pc: &Option<PostCondition>) {
        if let Some(pc) = pc
            && !self
                .style
                .keywords
                .allows_postcondition_on_argument(command)
        {
            self.report(
                TypeErrorKind::ArgumentPostconditional,
                pc.start..pc.end,
                None,
            );
        }
    }

    fn block(&mut self, block: &Range<Block>, ctx: Ctx) {
        match block.deref() {
            Block::Curry(curry) => self.curry(curry, ctx),
            Block::Command(commands) => {
                for command in &commands.items {
                    self.command(command, ctx);
                }
            }
        }
    }

    fn curry<G>(&mut self, curry: &CurryBlock<G>, ctx: Ctx) {
        self.braces(curry, ctx);
        self.procedure_body(&curry.block.wrapped, ctx);
    }

    // A block's braces against the style: whether a line break sits between each pair of
    // neighbours the style has an opinion on. A `#dim` owns its line, and a dotted line its dots:
    // neither joins a brace.
    fn braces<G>(&mut self, curry: &CurryBlock<G>, ctx: Ctx) {
        let (open, close) = (curry.block.start, curry.block.end - 1);
        let commands: Vec<_> = body_commands(&curry.block.wrapped.main).collect();
        let (first, last) = (commands.first().copied(), commands.last().copied());
        let rules = self.style.rules;
        let (before, after) = rules
            .open_brace
            .map_or((None, None), |b| (b.newline_before, b.newline_after));
        let indent = self.line_indent(ctx.anchor);
        if let Some(wanted) = before {
            let kind = TypeErrorKind::OpenBraceBreak {
                wanted: Break(wanted),
            };
            self.gap(
                wanted,
                curry.gap.start..open,
                open..open + 1,
                kind,
                Some(indent),
                ctx,
            );
        }
        if let (Some(wanted), Some((first, dotted)), Some((last, _))) = (after, first, last) {
            // The body's own indentation when a line of it stands alone, else one level in.
            let own_line = (commands.windows(2))
                .any(|pair| self.slice(pair[0].0.end..pair[1].0.start).contains('\n'));
            let body_indent = if own_line {
                self.line_indent(last.start).to_string()
            } else {
                format!("{indent}{}", self.indent.unwrap_or("\t"))
            };
            let kind = TypeErrorKind::BodyBreak {
                wanted: Break(wanted),
            };
            let dim = matches!(**first, Command::Dim(_));
            let fix = (!dotted && !dim).then_some(body_indent.as_str());
            self.gap(
                wanted,
                open + 1..first.start,
                first.start..first.end,
                kind,
                fix,
                ctx,
            );
        }
        if let (Some(wanted), Some((last, dotted))) =
            (rules.close_brace.and_then(|b| b.newline_before), last)
        {
            let kind = TypeErrorKind::CloseBraceBreak {
                wanted: Break(wanted),
            };
            let fix = (!dotted && !matches!(**last, Command::Dim(_))).then_some(indent);
            self.gap(wanted, last.end..close, close..close + 1, kind, fix, ctx);
        }
    }

    // `Else`, `ElseIf` or `Catch` (`keyword`, at `kw`) after the block whose `}` is at `close`.
    fn chain<K>(&mut self, close: Option<usize>, kw: &Range<K>, keyword: &'static str, ctx: Ctx) {
        let newline_after = self.style.rules.close_brace.and_then(|b| b.newline_after);
        let (Some(close), Some(wanted)) = (close, newline_after) else {
            return;
        };
        let kind = TypeErrorKind::ChainBreak {
            keyword,
            wanted: Break(wanted),
        };
        let indent = Some(self.line_indent(ctx.anchor));
        self.gap(
            wanted,
            close + 1..kw.start,
            kw.start..kw.end,
            kind,
            indent,
            ctx,
        );
    }

    // Whether a line break in `span` is as `wanted`; if not, the finding at `at`, with its edit when
    // `fix` gives an added line's indentation and the span is whitespace alone: a comment or `#`
    // line in it stays where it is.
    fn gap(
        &mut self,
        wanted: bool,
        span: Span,
        at: Span,
        kind: TypeErrorKind,
        fix: Option<&str>,
        ctx: Ctx,
    ) {
        let between = self.slice(span.clone());
        if between.contains('\n') == wanted {
            return;
        }
        let edit = fix
            .filter(|_| between.trim().is_empty())
            .and_then(|indent| {
                let text = if wanted {
                    format!("{}{indent}", self.newline)
                } else {
                    " ".to_string()
                };
                self.edit(ctx, span, text)
            });
        self.report(kind, at, edit);
    }
}

fn commands_of<S>(line: &cos::CommandLine<S>) -> impl Iterator<Item = &Range<Command>> {
    line.commands
        .iter()
        .flat_map(|commands| commands.items.iter())
}

// A body's commands in order, each with whether its line is dotted.
fn body_commands(main: &cos::ProcedureMain) -> impl Iterator<Item = (&Range<Command>, bool)> {
    let line_1 = (main.line_1.iter())
        .flat_map(|line| commands_of(line).map(move |c| (c, !line.periods.is_empty())));
    let line_n = (main.line_n.iter())
        .flat_map(|line_n| line_n.items.iter())
        .flat_map(|line| commands_of(line).map(move |c| (c, !line.periods.is_empty())));
    line_1.chain(line_n)
}

// `Label+n`, with or without `^Routine`.
fn has_offset(entry: &cos::LegacyEntryRef) -> bool {
    matches!(entry, cos::LegacyEntryRef::Label(lor) if lor.offset.is_some())
}

// The offset of the `}` closing `block`, if it has braces.
fn close_brace_of(block: &Range<Block>) -> Option<usize> {
    matches!(block.deref(), Block::Curry(_)).then_some(block.end - 1)
}
