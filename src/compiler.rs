use crate::parser::{AshParser, AstNode};
use crate::vm::{ArgType, FnCall, VM};
use camino::Utf8PathBuf;
use std::{collections::HashMap, fs};

pub struct Compiler {
    path_to_fn: HashMap<Utf8PathBuf, String>,
    fn_to_path: HashMap<String, Utf8PathBuf>,
    fn_to_ast: HashMap<String, Vec<AstNode>>,
    block_fn_asm: Vec<Vec<String>>,
    block_counter: usize,
    pub print_asm: bool,
    // each function gets it's own memory space
    // track where in the memory we're at
    memory_offset: usize,
    called_fn: HashMap<FnCall, u64>,
    written_fn: HashMap<FnCall, bool>,
}

/**
 * The Compiler struct handles reading filepaths,
 * parsing files, recursively loading dependencies,
 * and then combining functions to form the final asm.
 *
 * Compiler uses many VM instances to compile individual functions.
 * Compiler is responsible for structuring each function asm into
 * a full output file.
 */
impl Compiler {
    pub fn new() -> Self {
        Compiler {
            path_to_fn: HashMap::new(),
            fn_to_path: HashMap::new(),
            fn_to_ast: HashMap::new(),
            block_fn_asm: Vec::new(),
            block_counter: 0,
            print_asm: false,
            memory_offset: 0,
            called_fn: HashMap::new(),
            written_fn: HashMap::new(),
        }
    }

    // builtin functions that are globally available
    //
    // files may not use these strings as names
    pub fn builtins() -> HashMap<String, Vec<String>> {
        let mut out = HashMap::new();

        // cause execution to fall off the bottom without halting
        out.insert("crash".to_string(), vec!["crash:".to_string()]);

        out
    }

    // include a path in the build
    //
    // if the include is a file, the function name is calculated
    // and stored in the local instance
    //
    // if the include is a directory, the directory is recursively
    // walked and passed to this function
    pub fn include(&mut self, path: Utf8PathBuf) {
        // first check if it's a directory
        let metadata = fs::metadata(&path)
            .unwrap_or_else(|_| panic!("Failed to stat metadata for include path: {:?}", path));
        if metadata.is_file() {
            let ext = path
                .extension()
                .unwrap_or_else(|| panic!("Failed to get extension for path: {:?}", path));
            if ext != "ash" {
                return;
            }
            let name_str = path
                .file_stem()
                .unwrap_or_else(|| panic!("Failed to parse file stem for include path: {:?}", path))
                .to_string();
            // if self.fn_to_path.contains_key(&name_str) {
            //     // skip for now
            //     println!("Duplicate file/function names detected: {name_str}");
            //     println!("Path 1: {:?}", &path);
            //     println!("Path 2: {:?}", self.fn_to_path.get(&name_str).unwrap());
            //     std::process::exit(1);
            // } else {
            self.fn_to_path.insert(name_str.clone(), path.clone());
            self.path_to_fn.insert(path, name_str);
            // }
        } else if metadata.is_dir() {
            let files = fs::read_dir(&path)
                .unwrap_or_else(|_| panic!("Failed to read directory: {:?}", &path));
            for entry in files {
                let next_path = entry
                    .unwrap_or_else(|_| panic!("Failed to read dir entry: {:?}", &path))
                    .path();
                self.include(Utf8PathBuf::from_path_buf(next_path).unwrap());
            }
        }
    }

    // transforms an ast into compiled assembly
    // accepts a reference to a map of function names
    // any functions called in the ast will be added
    // to the map
    fn ast_to_asm(
        &mut self,
        ast: Vec<AstNode>,
        vm_opt: Option<&mut VM>,
        arg_types: Vec<ArgType>,
    ) -> Vec<String> {
        let mut default_vm = VM::new(&mut self.memory_offset, &self.fn_to_ast);
        let vm = vm_opt.unwrap_or(&mut default_vm);
        for v in ast {
            match v {
                AstNode::Stmt(name, is_let, expr) => {
                    if is_let {
                        vm.let_var(name, expr);
                    } else {
                        vm.set_var(name, expr)
                    }
                }
                AstNode::FnVar(vars) => {
                    if arg_types.len() != vars.len() {
                        panic!(
                            "function argument count mismatch: expected {}, got {}",
                            arg_types.len(),
                            vars.len()
                        );
                    }
                    for x in 0..vars.len() {
                        vm.fn_var(vars[x].clone(), arg_types[x].clone());
                    }
                }
                AstNode::Rtrn(expr) => {
                    vm.return_expr(expr);
                }
                AstNode::Const(name, expr) => {
                    // we must be able to fully evaluate
                    // the constant at compile time
                    // e.g. the expr must contain only
                    // Expr::Lit and Expr::Val containing other consts
                    vm.const_var(name, expr);
                }
                AstNode::If(expr, block_ast) => {
                    vm.eval(expr);
                    let block_name = format!("block_{}", self.block_counter);
                    self.block_counter += 1;
                    vm.call_block(&block_name);
                    // vm.eval(expr1);
                    // vm.eval(expr2);
                    // push 0 to the stack based on the bool_op
                    let mut block_vm = VM::from_vm(&(*vm));
                    // let block_asm =
                    block_vm.begin_block();
                    //
                    self.ast_to_asm(block_ast, Some(&mut block_vm), vec![]);

                    block_vm.end_block();
                    let mut block_asm: Vec<String> = Vec::new();
                    block_asm.push(format!("{block_name}:"));
                    block_asm.append(&mut block_vm.asm);
                    block_asm.push("return".to_string());
                    self.block_fn_asm.push(block_asm);
                }
            }
        }
        for (call, count) in &vm.fn_calls {
            if !self.called_fn.contains_key(call) {
                self.called_fn.insert(call.clone(), count.clone());
            } else {
                self.called_fn
                    .entry(call.clone())
                    .and_modify(|v| *v += count);
            }
        }
        vm.asm.clone()
    }

    // loads, parses, and returns an ashlang function by name
    // returns the function as an ast
    pub fn parse_fn(&self, fn_name: &String) -> AshParser {
        if let Some(file_path) = self.fn_to_path.get(fn_name) {
            let unparsed_file = std::fs::read_to_string(file_path)
                .unwrap_or_else(|_| panic!("Failed to read source file: {:?}", file_path));
            // let the parser throw it's error to stderr/out
            AshParser::parse(&unparsed_file)
        } else {
            panic!("function is not present in sources: {fn_name}");
        }
    }

    // start at the entry file
    // parse it and determine what other files are needed
    // repeat until all files have been parsed
    pub fn compile(&mut self, entry: &Utf8PathBuf) -> String {
        let entry_fn_name = entry.file_stem().unwrap().to_string();

        let parser = self.parse_fn(&entry_fn_name);

        // tracks total number of includes for a fn in all sources
        let mut included_fn: HashMap<String, u64> = parser.fn_names.clone();
        // let mut fn_arg_types: HashMap<String, Vec<ArgType>> = HashMap::new();
        let builtins = Compiler::builtins();
        for (name, _v) in builtins.iter() {
            included_fn.insert(name.clone(), 0);
            self.fn_to_ast.insert(name.clone(), vec![]);
        }
        // step 1: build ast for all functions
        // each function has a single ast, but multiple implementations
        // based on argument types it is called with
        loop {
            if included_fn.len() == self.fn_to_ast.len() {
                break;
            }
            for (fn_name, _) in included_fn.clone() {
                if self.fn_to_ast.contains_key(&fn_name) {
                    continue;
                }
                let parser = self.parse_fn(&fn_name);
                for (fn_name, count) in parser.fn_names {
                    if let Some(x) = included_fn.get_mut(&fn_name) {
                        *x += count;
                    } else {
                        included_fn.insert(fn_name, count);
                    }
                }
                self.fn_to_ast.insert(fn_name, parser.ast);
            }
        }

        // step 1: compile the entrypoint to assembly
        let mut asm = self.ast_to_asm(parser.ast, None, vec![]);
        asm.push("halt".to_string());

        // step 2: compile each function variant and insert into the asm
        loop {
            for (fn_call, _) in self.called_fn.clone() {
                if builtins.contains_key(&fn_call.name) {
                    self.written_fn.insert(fn_call, true);
                    continue;
                }
                if self.written_fn.contains_key(&fn_call) {
                    continue;
                }
                // otherwise iterate over the invocation argument configurations
                let mut vm = VM::new(&mut self.memory_offset, &self.fn_to_ast);
                let ast = self.fn_to_ast.get(&fn_call.name).unwrap();
                self.ast_to_asm(ast.clone(), Some(&mut vm), fn_call.arg_types.clone());
                vm.return_if_needed();
                asm.push("\n".to_string());
                // build the custom fn name
                asm.push(format!("{}:", fn_call.typed_name()));
                asm.append(&mut vm.asm);
                asm.push("return".to_string());
                self.written_fn.insert(fn_call, true);
            }
            if self.called_fn.len() == self.written_fn.len() {
                break;
            }
        }
        // step 3: add blocks to file
        for v in self.block_fn_asm.iter() {
            let mut block_asm = v.clone();
            asm.push("\n".to_string());
            asm.append(&mut block_asm);
        }

        // step 4: add builtin functions
        for asms in builtins.values() {
            asm.push("\n".to_string());
            let mut a = asms.clone();
            asm.append(&mut a);
        }
        if self.print_asm {
            // prints the assembly
            for l in &asm {
                println!("{}", l);
            }
        }
        asm.clone().join("\n")
    }
}
