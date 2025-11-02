use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{Result, anyhow, bail};

use crate::cli::commands::util::{lookup_named_cid, parse_type_tags};
use march5::{TypeTag, open_store};

pub(crate) fn cmd_builder(store: &Path) -> Result<()> {
    let conn = open_store(store)?;
    let mut builder = march5::GraphBuilder::new(&conn);
    let stdin = io::stdin();
    let mut input = String::new();
    let mut current_params: Option<Vec<TypeTag>> = None;

    println!(
        "March builder REPL. Commands: begin, begin-guard, lit, prim, call, dup, swap, over, attach-guard <name|cid>, stack, finish, finish-guard, reset, help, quit."
    );
    loop {
        print!("builder> ");
        io::stdout().flush().ok();
        input.clear();
        if stdin.lock().read_line(&mut input)? == 0 {
            break;
        }
        let line = input.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap();
        let remaining: Vec<&str> = parts.collect();

        let result = match cmd {
            "help" => {
                println!(
                    "Commands:\n  begin [types...]             start a word with parameter types (e.g. begin i64 i64)\n  begin-guard [types...]       start a guard with parameter types\n  lit <i64>                    push literal\n  prim <primCID|name>          apply primitive by CID or name\n  call <wordCID|name>          call existing word by CID or name\n  dup|swap|over                stack shuffles\n  attach-guard <name|cid>      attach a guard to current word\n  stack                        show current stack depth\n  finish <result> [name]       finish word with result type and optional name\n  finish-guard <result> [name] finish guard with result type (i64 expected) and optional name\n  reset                        abandon current build\n  quit/exit                    leave the REPL"
                );
                Ok(())
            }
            "quit" | "exit" => break,
            "begin" => {
                let tags =
                    parse_type_tags(&remaining.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
                builder.begin_word(&tags)?;
                current_params = Some(tags);
                println!(
                    "began word with {} parameter(s)",
                    current_params.as_ref().unwrap().len()
                );
                Ok(())
            }
            "begin-guard" => {
                let tags =
                    parse_type_tags(&remaining.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
                builder.begin_guard(&tags)?;
                current_params = Some(tags);
                println!(
                    "began guard with {} parameter(s)",
                    current_params.as_ref().unwrap().len()
                );
                Ok(())
            }
            "reset" => {
                builder.begin_word(&[])?;
                current_params = Some(Vec::new());
                println!("state reset");
                Ok(())
            }
            "lit" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.len() != 1 {
                    bail!("lit expects one argument");
                }
                let value: i64 = remaining[0].parse()?;
                builder.push_lit_i64(value)?;
                Ok(())
            }
            "prim" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.len() != 1 {
                    bail!("prim expects CID or name");
                }
                let cid = lookup_named_cid(&conn, "prim", remaining[0])?;
                builder.apply_prim(cid)?;
                Ok(())
            }
            "call" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.len() != 1 {
                    bail!("call expects CID or name");
                }
                let cid = lookup_named_cid(&conn, "word", remaining[0])?;
                builder.apply_word(cid)?;
                Ok(())
            }
            "attach-guard" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.len() != 1 {
                    bail!("attach-guard expects a guard name or CID");
                }
                let cid = lookup_named_cid(&conn, "guard", remaining[0])?;
                builder.attach_guard(cid);
                Ok(())
            }
            "dup" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                builder.dup()
            }
            "swap" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                builder.swap()
            }
            "over" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                builder.over()
            }
            "stack" => {
                println!("stack depth: {}", builder.depth());
                Ok(())
            }
            "finish" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.is_empty() {
                    bail!("finish requires a result type, e.g., finish i64 [name]");
                }
                let result_tag = TypeTag::from_atom(remaining[0])?;
                let name = if remaining.len() > 1 {
                    Some(remaining[1].to_string())
                } else {
                    None
                };
                let params = current_params
                    .as_ref()
                    .ok_or_else(|| anyhow!("no word in progress; use begin first"))?;
                let cid = builder.finish_word(params, &[result_tag], name.as_deref())?;
                println!("stored word with cid {}", march5::cid::to_hex(&cid));
                current_params = None;
                Ok(())
            }
            "finish-guard" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.is_empty() {
                    bail!("finish-guard requires a result type (i64) and optional name");
                }
                let result_tag = TypeTag::from_atom(remaining[0])?;
                let name = if remaining.len() > 1 {
                    Some(remaining[1].to_string())
                } else {
                    None
                };
                let params = current_params
                    .as_ref()
                    .ok_or_else(|| anyhow!("no guard in progress; use begin-guard first"))?;
                let cid = builder.finish_guard(params, &[result_tag], name.as_deref())?;
                println!("stored guard with cid {}", march5::cid::to_hex(&cid));
                current_params = None;
                Ok(())
            }
            _ => bail!("unknown command `{cmd}`"),
        };

        if let Err(err) = result {
            eprintln!("error: {err}");
        }
    }

    Ok(())
}

fn ensure_builder_begun(
    builder: &mut march5::GraphBuilder<'_>,
    current_params: &mut Option<Vec<TypeTag>>,
) -> Result<()> {
    if current_params.is_none() {
        builder.begin_word(&[])?;
        *current_params = Some(Vec::new());
    }
    Ok(())
}
