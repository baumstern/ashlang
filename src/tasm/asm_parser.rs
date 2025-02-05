use anyhow::Result;
use pest::Parser;
use pest_derive::Parser;

use crate::log;
use crate::tasm::vm::ArgType;
use crate::tasm::vm::FnCall;
use crate::tasm::vm::VarLocation;
use log::error;

#[derive(Parser)]
#[grammar = "tasm/tasm_grammar.pest"] // relative to project `src`
pub struct AsmPestParser;

pub struct AsmParser {
    pub call_type: FnCall,
    pub asm: Vec<String>,
}

impl AsmParser {
    pub fn parse(source: &str, name: &str) -> Result<Self> {
        let mut call_type = None;
        let mut asm = Vec::new();
        let parsed = AsmPestParser::parse(Rule::program, source)?;
        for pair in parsed {
            match pair.as_rule() {
                Rule::type_header => {
                    let pair = pair.into_inner();
                    let mut arg_types: Vec<ArgType> = Vec::new();
                    for v in pair {
                        match v.as_rule() {
                            Rule::scalar => {
                                arg_types.push(ArgType {
                                    location: VarLocation::Stack,
                                    dimensions: vec![],
                                    value: None,
                                });
                            }
                            Rule::dimension => {
                                let pair = v.into_inner();
                                let mut dimensions = vec![];
                                for z in pair {
                                    dimensions.push(z.as_str().parse::<usize>().unwrap());
                                }
                                arg_types.push(ArgType {
                                    location: VarLocation::Memory,
                                    dimensions,
                                    value: None,
                                });
                            }
                            _ => {
                                return Err(anyhow::anyhow!(
                                    "unexpected type_header rule: {:?}",
                                    v.as_rule()
                                ));
                            }
                        }
                    }
                    if arg_types.is_empty() {
                        return Err(anyhow::anyhow!("unexpected: bad arg types"));
                    }
                    let return_type = arg_types.pop().unwrap();
                    call_type = Some(FnCall {
                        name: name.to_string(),
                        arg_types,
                        return_type: Some(return_type),
                    });
                }
                Rule::stmt => {
                    let mut pair = pair.into_inner();
                    if let Some(next) = pair.next() {
                        asm.push(next.as_str().to_string());
                    }
                }
                Rule::EOI => {}
                _ => {
                    return Err(anyhow::anyhow!(
                        "unexpected line pair rule: {:?}",
                        pair.as_rule()
                    ));
                }
            }
        }
        if let Some(call_type) = call_type {
            Ok(Self { call_type, asm })
        } else {
            error!("No type header found in asm file")
        }
    }
}
