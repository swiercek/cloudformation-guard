use std::convert::TryFrom;
use std::fs::File;
use std::path::PathBuf;

use clap::{App, Arg, ArgMatches};
use colored::*;

use crate::command::Command;
use crate::commands::{ALPHABETICAL, LAST_MODIFIED};
use crate::commands::files::{alpabetical, get_files, last_modified, read_file_content, regular_ordering};
use crate::rules::{Evaluate, EvaluationContext, Result, Status, EvaluationType};
use crate::rules::errors::{Error, ErrorKind};
use crate::rules::evaluate::RootScope;
use crate::rules::exprs::RulesFile;
use crate::rules::values::Value;
use nom::lib::std::collections::HashMap;
use crate::rules::path_value::PathAwareValue;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Validate {}

impl Validate {
    pub(crate) fn new() -> Self {
        Validate{}
    }
}

impl Command for Validate {
    fn name(&self) -> &'static str {
        "validate"
    }


    fn command(&self) -> App<'static, 'static> {
        App::new("validate")
            .about(r#"
             Evaluates rules against the data files to determine
             success or failure. When pointed to a directory it will
             read all rules in the directory file and evaluate them
             against the data files found in the directory. The command
             can also point to a single file and it would work as well
        "#)
            .arg(Arg::with_name("rules").long("rules").short("r").takes_value(true).help("provide a rules file or a directory").required(true))
            .arg(Arg::with_name("data").long("data").short("d").takes_value(true).help("provide a file or dir for data files in JSON or YAML").required(true))
            .arg(Arg::with_name("alphabetical").alias("-a").help("sort alphabetically inside a directory").required(false))
            .arg(Arg::with_name("last-modified").short("-m").required(false).conflicts_with("alphabetical")
                .help("sort by last modified times within a directory"))
            .arg(Arg::with_name("verbose").long("verbose").short("v").required(false)
                .help("verbose logging"))
    }

    fn execute(&self, app: &ArgMatches<'_>) -> Result<()> {
        let file = app.value_of("rules").unwrap();
        let data = app.value_of("data").unwrap();
        let cmp = if let Some(_ignored) = app.value_of(ALPHABETICAL.0) {
            alpabetical
        } else if let Some(_ignored) = app.value_of(LAST_MODIFIED.0) {
            last_modified
        } else {
            regular_ordering
        };

        let verbose = if app.is_present("verbose") {
            true
        } else {
            false
        };


        let files = get_files(file, cmp)?;
        let data_files = get_files(data, cmp)?;

        for each_rule_file in files {
            match open_file(&each_rule_file) {
                Ok((rule_file_name, file)) => {
                    match read_file_content(file) {
                        Ok(file_content) => {
                            let span = crate::rules::parser::Span::new_extra(&file_content, &rule_file_name);
                            match crate::rules::parser::rules_file(span) {
                                Err(e) => {
                                    println!("Parsing error handling rule file = {}, Error = {}",
                                             rule_file_name.underline(), e);
                                    continue;
                                },

                                Ok(rules) => {
                                    evaluate_against_data_files(&data_files, &rules, verbose)?
                                }
                            }
                        },

                        Err(e) => {
                            let msg = format!("Error = {}", e);
                            let msg = msg.red();
                            println!("Unable to process file {} Error = {}", rule_file_name, msg);
                            continue;
                        }
                    }
                },

                Err(e) => {
                    let msg = format!("Unable to open file {}, Error = {}",
                                      each_rule_file.display().to_string().underline(), e);
                    println!("{}", msg);
                    continue;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
struct StatusContext {
    eval_type: EvaluationType,
    context: String,
    indent: usize,
    msg: Option<String>,
    from: Option<PathAwareValue>,
    to: Option<PathAwareValue>,
    status: Option<Status>
}

impl StatusContext {
    fn new(eval_type: EvaluationType, context: &str, depth: usize) -> Self {
        StatusContext {
            eval_type,
            context: context.to_string(),
            status: None,
            msg: None,
            from: None,
            to: None,
            indent: depth
        }
    }
}


#[derive(Debug)]
struct ContainerContext {
    status_cxt: StatusContext,
    children: Vec<StackContext>,
}

impl ContainerContext {
    fn new(eval_type: EvaluationType, context: &str, depth: usize) -> Self {
        ContainerContext {
            status_cxt: StatusContext::new(eval_type, context, depth),
            children: vec![]
        }
    }
}

#[derive(Debug)]
enum StackContext {
    Single(StatusContext),
    Container(ContainerContext)
}

#[derive(Debug)]
struct ConsoleReporter<'r,'loc>{
    root_context: &'r RootScope<'r, 'loc>,
    stack: std::cell::RefCell<Vec<StackContext>>,
    verbose: bool
}

fn colored_string(status: Option<Status>) -> ColoredString {
    let status = match status {
        Some(s) => s,
        None => Status::SKIP,
    };
    match status {
        Status::PASS => "PASS".green(),
        Status::FAIL => "FAIL".red().bold(),
        Status::SKIP => "SKIP".yellow().bold(),
    }
}

fn indent_spaces(indent: usize) {
    for _idx in 0..indent {
        print!("{}", INDENT)
    }
}

fn print_context(current: &StackContext) {
    let cxt = match current {
        StackContext::Single(cxt) => cxt,
        StackContext::Container(cxt) => &cxt.status_cxt
    };

    let header = format!("{}({}, {})", cxt.eval_type, cxt.context, colored_string(cxt.status)).underline();
    let depth = cxt.indent;
    let sub_indent = depth + 1;
    indent_spaces(depth - 1);
    println!("{}", header);
    match &cxt.from {
        Some(v) => {
            indent_spaces(depth);
            print!("|  ");
            println!("From: {:?}", v);
        },
        None => {}
    }
    match &cxt.to {
        Some(v) => {
            indent_spaces(depth);
            print!("|  ");
            println!("To: {:?}", v);
        },
        None => {}
    }

    if let StackContext::Container(container) = current {
        for child in &container.children {
            print_context(child)
        }
    }
}

impl<'r, 'loc> ConsoleReporter<'r, 'loc> {
    fn new(root: &'r RootScope<'r, 'loc>, verbose: bool) -> Self {
        ConsoleReporter {
            root_context: root,
            stack: std::cell::RefCell::new(Vec::new()),
            verbose,
        }
    }

    fn report(self) {
        print!("{}", "Summary Report".underline());
        let top = match self.stack.borrow_mut().pop().unwrap() {
            StackContext::Container(c) => c,
            _ => unreachable!()
        };

        println!(" Overall File Status = {}", colored_string(top.status_cxt.status));

        let longest = top.children.iter()
            .map(|elem| match elem {
                StackContext::Single(single) => &single.context,
                StackContext::Container(container) => &container.status_cxt.context
            })
            .max_by(|f, s| {
                (*f).len().cmp(&(*s).len())
            })
            .map(|elem| elem.len())
            .unwrap_or(20);

       for each in &top.children {
            match each {
                StackContext::Container(container) => {
                    print!("{}", container.status_cxt.context);
                    let container_level = container.status_cxt.context.len();
                    let spaces = longest - container_level + 4;
                    for _idx in 0..spaces {
                        print!(" ");
                    }
                    println!("{}", colored_string(container.status_cxt.status));
                },
                _ => {}
            }
        }

        if self.verbose {
            println!("Evaluation Tree");
            for each in &top.children {
                print_context(each);
            }
        }
    }
}

const INDENT: &str = "    ";

impl<'r, 'loc> EvaluationContext for ConsoleReporter<'r, 'loc> {
    fn resolve_variable(&self, variable: &str) -> Result<Vec<&PathAwareValue>> {
        self.root_context.resolve_variable(variable)
    }

    fn rule_status(&self, rule_name: &str) -> Result<Status> {
        self.root_context.rule_status(rule_name)
    }

    fn end_evaluation(&self,
                      eval_type: EvaluationType,
                      context: &str,
                      msg: String,
                      from: Option<PathAwareValue>,
                      to: Option<PathAwareValue>,
                      status: Option<Status>) {

        if self.stack.borrow().len() == 1 {
            match self.stack.borrow_mut().get_mut(0) {
                Some(top) => {
                    match top {
                        StackContext::Container(c) => {
                            c.status_cxt.status = status.clone();
                            c.status_cxt.from = from.clone();
                            c.status_cxt.to = to.clone();
                            c.status_cxt.msg = Some(msg.clone());
                        },
                        _ => {}
                    }
                },

                None => unreachable!()
            }
            return;
        }

        let stack = self.stack.borrow_mut().pop();
        match stack {
            Some(mut stack) => {
                let status_cxt = match &mut stack {
                    StackContext::Container(c) => {
                        &mut c.status_cxt
                    },
                    StackContext::Single(c) => c
                };
                status_cxt.status = status.clone();
                status_cxt.from = from.clone();
                status_cxt.to = to.clone();
                status_cxt.msg = Some(msg.clone());

                match self.stack.borrow_mut().last_mut() {
                    Some(cxt) =>  {
                        match cxt {
                            StackContext::Container(container) => {
                                container.children.push(stack)
                            },

                            _ => unreachable!()
                        }
                    }
                    None => unreachable!()
                }
            },
            None => {}
        }
        self.root_context.end_evaluation(eval_type, context, msg, from, to, status);
    }

    fn start_evaluation(&self,
                        eval_type: EvaluationType,
                        context: &str) {
        let indent= self.stack.borrow().len();
        self.stack.borrow_mut().push(
            match eval_type {
                EvaluationType::Clause => StackContext::Single(StatusContext::new(eval_type, context, indent)),
                _ => StackContext::Container(ContainerContext::new(eval_type, context, indent))
            }
        );
        self.root_context.start_evaluation(eval_type, context);
    }

}

impl<'r, 'loc> ConsoleReporter<'r, 'loc> {
    fn colorized(eval_type: EvaluationType, context: &str) {
        match eval_type {
            EvaluationType::Rule => println!("{}", format!("{} = {}", eval_type, context).truecolor(200, 170, 217).underline()),
            EvaluationType::Type => println!("{}", format!("{} = {}", eval_type, context).truecolor(192, 80, 47).underline()),
            EvaluationType::Condition => println!("{}", format!("when@{}", context).truecolor(183, 178, 79).underline()),
            EvaluationType::Filter => println!("{}", "Filter".truecolor(109, 104, 15).underline()),
            EvaluationType::Clause => println!("{}", format!("Clause = {}", context).truecolor(63, 147, 63).underline()),
            _ => println!("{}/{}", eval_type, context)
        }
    }

}



fn evaluate_against_data_files(data_files: &[PathBuf], rules: &RulesFile<'_>, verbose: bool) -> Result<()> {
    for each in data_files {
        match open_file(each) {
            Ok((name, file)) => {
                match read_file_content(file) {
                    Ok(content) => {
                        let root = match serde_json::from_str::<serde_json::Value>(&content) {
                            Ok(value) => PathAwareValue::try_from(value)?,
                            Err(_) => {
                                let value = serde_yaml::from_str::<serde_json::Value>(&content)?;
                                PathAwareValue::try_from(value)?
                            }
                        };

                        let root_context = RootScope::new(rules, &root);
                        let reporter = ConsoleReporter::new(&root_context, verbose);
                        rules.evaluate(&root, &reporter)?;
                        reporter.report();
                        //summary_report(&root_context);
                        //println!("{:?}", reporter);
                        //root_context.summary_report();
                    },

                    Err(e) => {
                        println!("Unable to process data file = {}, Error = {}", name.underline(), e);
                    }
                }
            },

            Err(e) => {
                println!("Unable to open data file = {}, Error = {}", each.display(), e);
            }
        }

    }
    Ok(())
}

fn summary_report(root_scope: &RootScope<'_, '_>) {
    println!("{}", "Summary Report".underline());
    let mut longest = 0;
    let mut find_longest = |name: &str, _status: &Status| {
        if name.len() > longest {
            longest = name.len()
        }
    };
    root_scope.rule_statues(find_longest);
    let output = |name: &str, status: &Status| {
        let status = match status {
            Status::PASS => "PASS".green(),
            Status::FAIL => "FAIL".red(),
            Status::SKIP => "SKIP".yellow(),
        };
        print!("{}", name);
        for _idx in 0..(longest + 2 - name.len()) {
            print!("{}", "    ");
        }
        println!("{}", status);
    };
    root_scope.rule_statues(output);

//    for each in rule_statues.iter() {
//        let status = match *each.1 {
//            Status::PASS => "PASS".green(),
//            Status::FAIL => "FAIL".red(),
//            Status::SKIP => "SKIP".yellow(),
//        };
//        print!("{}", *each.0);
//        for _idx in 0..(longest + 2 - (*each.0).len()) {
//            print!("{}", "    ");
//        }
//        println!("{}", status);
//    }
}


fn open_file(path: &PathBuf) -> Result<(String, std::fs::File)> {
    if let Some(file_name) = path.file_name() {
        if let Some(file_name) = file_name.to_str() {
            let file_name = file_name.to_string();
            return Ok((file_name, File::open(path)?))
        }
    }
    Err(Error::new(
        ErrorKind::IoError(std::io::Error::from(std::io::ErrorKind::NotFound))))
}
