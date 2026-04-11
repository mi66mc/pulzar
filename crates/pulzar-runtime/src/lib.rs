use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use pulzar_builtins::{BuiltinId, BuiltinSpec, format_usage, render_help, resolve_segments};
use pulzar_syntax::{
    BinaryOp, Block, Diagnostic, DiagnosticKind, Expr, ExprKind, File, FnBody, LambdaBody, Span,
    Stmt, StmtKind, UnaryOp,
};

#[derive(Debug, Clone)]
pub struct RuntimeResult {
    pub value: Option<Value>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct ShellContext {
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub interactive: bool,
}

#[derive(Debug)]
pub struct Session {
    shell: ShellContext,
    env: EnvRef,
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static INTERRUPT_HANDLER: OnceLock<()> = OnceLock::new();

impl Default for ShellContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            env: std::env::vars().collect(),
            interactive: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Object(BTreeMap<String, Value>),
    Function(Rc<Function>),
}

#[derive(Debug, Clone)]
pub struct Function {
    params: Vec<String>,
    body: FunctionBody,
    env: EnvRef,
}

#[derive(Debug, Clone)]
enum FunctionBody {
    Expr(Expr),
    Block(Block),
}

type EnvRef = Rc<Environment>;

#[derive(Debug)]
struct Environment {
    parent: Option<EnvRef>,
    bindings: RefCell<HashMap<String, Rc<RefCell<Value>>>>,
}

#[derive(Debug)]
enum Flow {
    Value(Value),
    Return(Value),
}

pub fn run_file(file: &File, shell: &mut ShellContext) -> RuntimeResult {
    let env = Environment::new(None);
    run_file_with_env(file, shell, env)
}

pub fn install_interrupt_handler() -> Result<(), String> {
    if INTERRUPT_HANDLER.get().is_some() {
        return Ok(());
    }

    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .map_err(|err| err.to_string())?;

    let _ = INTERRUPT_HANDLER.set(());
    Ok(())
}

pub fn take_interrupt() -> bool {
    INTERRUPTED.swap(false, Ordering::SeqCst)
}

pub fn run_file_in_session(file: &File, session: &mut Session) -> RuntimeResult {
    run_file_with_env(file, &mut session.shell, session.env.clone())
}

fn run_file_with_env(file: &File, shell: &mut ShellContext, env: EnvRef) -> RuntimeResult {
    let mut runtime = Runtime {
        shell,
        diagnostics: Vec::new(),
    };

    let value = match runtime.eval_file(file, env) {
        Ok(value) => Some(value),
        Err(diag) => {
            runtime.diagnostics.push(diag);
            None
        }
    };

    RuntimeResult {
        value,
        diagnostics: runtime.diagnostics,
    }
}

impl Session {
    pub fn new(shell: ShellContext) -> Self {
        Self {
            shell,
            env: Environment::new(None),
        }
    }

    pub fn cwd(&self) -> &Path {
        &self.shell.cwd
    }

    pub fn shell(&self) -> &ShellContext {
        &self.shell
    }

    pub fn shell_mut(&mut self) -> &mut ShellContext {
        &mut self.shell
    }
}

struct Runtime<'a> {
    shell: &'a mut ShellContext,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Runtime<'a> {
    fn eval_file(&mut self, file: &File, env: EnvRef) -> Result<Value, Diagnostic> {
        let mut last = Value::Null;
        for stmt in &file.statements {
            match self.eval_stmt(stmt, env.clone())? {
                Flow::Value(value) => last = value,
                Flow::Return(value) => return Ok(value),
            }
        }
        Ok(last)
    }

    fn eval_block(&mut self, block: &Block, parent: EnvRef) -> Result<Flow, Diagnostic> {
        let env = Environment::new(Some(parent));
        let mut last = Value::Null;
        for stmt in &block.statements {
            match self.eval_stmt(stmt, env.clone())? {
                Flow::Value(value) => last = value,
                Flow::Return(value) => return Ok(Flow::Return(value)),
            }
        }
        Ok(Flow::Value(last))
    }

    fn eval_stmt(&mut self, stmt: &Stmt, env: EnvRef) -> Result<Flow, Diagnostic> {
        match &stmt.kind {
            StmtKind::Let { name, value } => {
                let value = self.eval_expr(value, env.clone())?;
                env.define(name.clone(), value.clone());
                Ok(Flow::Value(value))
            }
            StmtKind::Assign { target, value } => {
                let value = self.eval_expr(value, env.clone())?;
                match &target.kind {
                    ExprKind::Variable(name) => {
                        if let Some(cell) = env.resolve(name) {
                            *cell.borrow_mut() = value.clone();
                            Ok(Flow::Value(value))
                        } else {
                            Err(runtime_error(
                                target.span,
                                format!("cannot assign to undeclared name `{name}`"),
                            ))
                        }
                    }
                    ExprKind::EnvVar(name) => {
                        self.shell.env.insert(name.clone(), stringify_value(&value));
                        Ok(Flow::Value(value))
                    }
                    _ => Err(runtime_error(target.span, "invalid assignment target")),
                }
            }
            StmtKind::FnDecl { name, params, body } => {
                let function = Value::Function(Rc::new(Function {
                    params: params.iter().map(|param| param.name.clone()).collect(),
                    body: match body {
                        FnBody::Expr(expr) => FunctionBody::Expr((**expr).clone()),
                        FnBody::Block(block) => FunctionBody::Block(block.clone()),
                    },
                    env: env.clone(),
                }));
                env.define(name.clone(), function.clone());
                Ok(Flow::Value(function))
            }
            StmtKind::Return { value } => {
                let value = match value {
                    Some(expr) => self.eval_expr(expr, env)?,
                    None => Value::Null,
                };
                Ok(Flow::Return(value))
            }
            StmtKind::Expr(expr) => {
                if matches!(expr.kind, ExprKind::Bareword(_)) {
                    Ok(Flow::Value(self.eval_command_expr(
                        expr,
                        &[],
                        None,
                        env,
                        stmt.span,
                    )?))
                } else {
                    Ok(Flow::Value(self.eval_expr(expr, env)?))
                }
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr, env: EnvRef) -> Result<Value, Diagnostic> {
        match &expr.kind {
            ExprKind::Bareword(text) => Ok(Value::String(text.clone())),
            ExprKind::Variable(name) => self.resolve_variable_value(name, expr.span, env),
            ExprKind::EnvVar(name) => Ok(self
                .shell
                .env
                .get(name)
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null)),
            ExprKind::Integer(value) => Ok(Value::Int(*value)),
            ExprKind::Float(value) => Ok(Value::Float(*value)),
            ExprKind::String(value) => Ok(Value::String(value.clone())),
            ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            ExprKind::List(items) => items
                .iter()
                .map(|item| self.eval_expr(item, env.clone()))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            ExprKind::Object(fields) => {
                let mut object = BTreeMap::new();
                for field in fields {
                    let value = self.eval_expr(&field.value, env.clone())?;
                    object.insert(field.name.clone(), value);
                }
                Ok(Value::Object(object))
            }
            ExprKind::Call { callee, args } => self.eval_call(callee, args, None, env, expr.span),
            ExprKind::Pipeline { left, right } => {
                let value = self.eval_expr(left, env.clone())?;
                self.eval_pipeline_stage(right, value, env, expr.span)
            }
            ExprKind::Lambda { params, body } => Ok(Value::Function(Rc::new(Function {
                params: params.iter().map(|param| param.name.clone()).collect(),
                body: match body {
                    LambdaBody::Expr(expr) => FunctionBody::Expr((**expr).clone()),
                    LambdaBody::Block(block) => FunctionBody::Block(block.clone()),
                },
                env,
            }))),
            ExprKind::Unary { op, expr: inner } => {
                let value = self.eval_expr(inner, env)?;
                self.eval_unary(*op, value, expr.span)
            }
            ExprKind::Binary { op, left, right } => {
                let left = self.eval_expr(left, env.clone())?;
                let right = self.eval_expr(right, env)?;
                self.eval_binary(*op, left, right, expr.span)
            }
            ExprKind::Member { object, fields } => {
                let mut value = self.eval_expr(object, env)?;
                for field in fields {
                    match value {
                        Value::Object(ref object) => {
                            value = object.get(field).cloned().ok_or_else(|| {
                                runtime_error(expr.span, format!("missing object field `{field}`"))
                            })?;
                        }
                        _ => return Err(runtime_error(expr.span, "member access on non-object")),
                    }
                }
                Ok(value)
            }
            ExprKind::Grouped(inner) => self.eval_expr(inner, env),
            ExprKind::Error => Err(runtime_error(
                expr.span,
                "cannot execute invalid expression",
            )),
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        pipeline_input: Option<Value>,
        env: EnvRef,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        if matches!(callee.kind, ExprKind::Bareword(_)) {
            return self.eval_command_expr(callee, args, pipeline_input, env, span);
        }

        let mut values = Vec::new();
        if let Some(input) = pipeline_input {
            values.push(input);
        }
        for arg in args {
            values.push(self.eval_expr(arg, env.clone())?);
        }

        let function = self.resolve_function_callable(callee, env)?;
        self.call_function(function, values, span)
    }

    fn eval_pipeline_stage(
        &mut self,
        stage: &Expr,
        input: Value,
        env: EnvRef,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        match &stage.kind {
            ExprKind::Call { callee, args } => self.eval_call(callee, args, Some(input), env, span),
            ExprKind::Bareword(_) => self.eval_command_expr(stage, &[], Some(input), env, span),
            ExprKind::Variable(_) => {
                let function = self.resolve_function_callable(stage, env)?;
                self.call_function(function, vec![input], span)
            }
            _ => {
                let callee = self.eval_expr(stage, env)?;
                match callee {
                    Value::Function(function) => self.call_function(function, vec![input], span),
                    _ => Err(runtime_error(
                        stage.span,
                        "pipeline stage is not callable or executable",
                    )),
                }
            }
        }
    }

    fn resolve_variable_value(
        &mut self,
        name: &str,
        span: Span,
        env: EnvRef,
    ) -> Result<Value, Diagnostic> {
        if let Some(cell) = env.resolve(name) {
            return Ok(cell.borrow().clone());
        }
        Err(runtime_error(
            span,
            format!(
                "unknown variable `{name}`; variables must be referenced with `$` after declaration"
            ),
        ))
    }

    fn resolve_function_callable(
        &mut self,
        callee: &Expr,
        env: EnvRef,
    ) -> Result<Rc<Function>, Diagnostic> {
        match &callee.kind {
            ExprKind::Variable(name) => {
                if let Some(cell) = env.resolve(name) {
                    match cell.borrow().clone() {
                        Value::Function(function) => Ok(function),
                        _ => Err(runtime_error(
                            callee.span,
                            format!("`{name}` is not callable"),
                        )),
                    }
                } else {
                    Err(runtime_error(
                        callee.span,
                        format!("unknown variable `{name}`"),
                    ))
                }
            }
            _ => match self.eval_expr(callee, env)? {
                Value::Function(function) => Ok(function),
                _ => Err(runtime_error(callee.span, "expression is not callable")),
            },
        }
    }

    fn call_function(
        &mut self,
        function: Rc<Function>,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        if function.params.len() != args.len() {
            return Err(runtime_error(
                span,
                format!(
                    "expected {} argument(s), got {}",
                    function.params.len(),
                    args.len()
                ),
            ));
        }

        let env = Environment::new(Some(function.env.clone()));
        for (param, value) in function.params.iter().zip(args) {
            env.define(param.clone(), value);
        }

        match &function.body {
            FunctionBody::Expr(expr) => self.eval_expr(expr, env),
            FunctionBody::Block(block) => match self.eval_block(block, env)? {
                Flow::Value(value) | Flow::Return(value) => Ok(value),
            },
        }
    }

    fn eval_command_expr(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        pipeline_input: Option<Value>,
        env: EnvRef,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let ExprKind::Bareword(command_name) = &callee.kind else {
            return Err(runtime_error(callee.span, "expected command name"));
        };

        if let Some((spec, consumed)) = self.resolve_builtin_command(command_name, args) {
            let start = consumed.saturating_sub(1);
            let mut values = Vec::new();
            if let Some(input) = pipeline_input {
                values.push(input);
            }
            for arg in &args[start..] {
                values.push(self.eval_expr(arg, env.clone())?);
            }
            return self.execute_builtin(spec, values, span);
        }

        let mut values = Vec::new();
        for arg in args {
            values.push(self.eval_expr(arg, env.clone())?);
        }
        self.run_external_command(command_name, values, pipeline_input, span)
    }

    fn resolve_builtin_command(
        &self,
        command_name: &str,
        args: &[Expr],
    ) -> Option<(&'static BuiltinSpec, usize)> {
        let mut segments = vec![command_name];
        for arg in args {
            match &arg.kind {
                ExprKind::Bareword(segment) => segments.push(segment.as_str()),
                _ => break,
            }
        }

        resolve_segments(&segments).map(|matched| (matched.spec, matched.consumed))
    }

    fn execute_builtin(
        &mut self,
        spec: &'static BuiltinSpec,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        self.ensure_builtin_arity(spec, args.len(), span)?;

        match spec.id {
            BuiltinId::Help => {
                if args.is_empty() {
                    return Ok(Value::String(render_help(None)));
                }

                let targets = args.iter().map(stringify_value).collect::<Vec<_>>();
                let target_refs = targets.iter().map(String::as_str).collect::<Vec<_>>();
                let matched = resolve_segments(&target_refs).ok_or_else(|| {
                    runtime_error(span, format!("unknown builtin `{}`", targets.join(" ")))
                })?;
                Ok(Value::String(render_help(Some(matched.spec))))
            }
            BuiltinId::FsRead => {
                let [path] = expect_arity(args, span)?;
                let path = stringify_value(&path);
                let path = self.shell.cwd.join(path);
                let contents = fs::read_to_string(&path).map_err(|err| {
                    runtime_error(span, format!("failed to read `{}`: {err}", path.display()))
                })?;
                Ok(Value::String(contents))
            }
            BuiltinId::FsPwd => Ok(Value::String(self.shell.cwd.display().to_string())),
            BuiltinId::FsLs => {
                let path = match args.as_slice() {
                    [] => self.shell.cwd.clone(),
                    [path] => self.shell.cwd.join(stringify_value(path)),
                    _ => unreachable!("arity checked above"),
                };

                let entries = fs::read_dir(&path).map_err(|err| {
                    runtime_error(span, format!("failed to list `{}`: {err}", path.display()))
                })?;

                let mut names = entries
                    .map(|entry| {
                        entry
                            .map(|entry| {
                                Value::String(entry.file_name().to_string_lossy().to_string())
                            })
                            .map_err(|err| {
                                runtime_error(span, format!("failed to read dir entry: {err}"))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                names.sort_by(|left, right| stringify_value(left).cmp(&stringify_value(right)));
                Ok(Value::List(names))
            }
            BuiltinId::Cd => {
                let target = match args.as_slice() {
                    [] => self
                        .shell
                        .env
                        .get("HOME")
                        .or_else(|| self.shell.env.get("USERPROFILE"))
                        .cloned()
                        .ok_or_else(|| {
                            runtime_error(span, "`cd` needs a path when HOME/USERPROFILE is unset")
                        })?,
                    [path] => stringify_value(path),
                    _ => unreachable!("arity checked above"),
                };
                let target = self.shell.cwd.join(target);
                let target = target.canonicalize().map_err(|err| {
                    runtime_error(
                        span,
                        format!(
                            "failed to change directory to `{}`: {err}",
                            target.display()
                        ),
                    )
                })?;
                self.shell.cwd = target.clone();
                Ok(Value::String(target.display().to_string()))
            }
            BuiltinId::EnvGet => {
                let [name] = expect_arity(args, span)?;
                let name = self.expect_env_name(name, span)?;
                Ok(self
                    .shell
                    .env
                    .get(&name)
                    .cloned()
                    .map(Value::String)
                    .unwrap_or(Value::Null))
            }
            BuiltinId::EnvSet => {
                let [name, value] = expect_arity(args, span)?;
                let name = self.expect_env_name(name, span)?;
                let value = stringify_value(&value);
                self.shell.env.insert(name, value.clone());
                Ok(Value::String(value))
            }
            BuiltinId::EnvUnset => {
                let [name] = expect_arity(args, span)?;
                let name = self.expect_env_name(name, span)?;
                self.shell.env.remove(&name);
                Ok(Value::Null)
            }
            BuiltinId::EnvList => {
                let mut out = BTreeMap::new();
                for (key, value) in &self.shell.env {
                    out.insert(key.clone(), Value::String(value.clone()));
                }
                Ok(Value::Object(out))
            }
            BuiltinId::Lines => {
                let [value] = expect_arity(args, span)?;
                match value {
                    Value::String(text) => Ok(Value::List(
                        text.lines()
                            .map(|line| Value::String(line.to_string()))
                            .collect(),
                    )),
                    _ => Err(runtime_error(span, "`lines` expects a string")),
                }
            }
            BuiltinId::Map => {
                let [list, callback] = expect_arity(args, span)?;
                let Value::List(items) = list else {
                    return Err(runtime_error(
                        span,
                        "`map` expects a list as first argument",
                    ));
                };
                let Value::Function(function) = callback else {
                    return Err(runtime_error(
                        span,
                        "`map` expects a function as second argument",
                    ));
                };

                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.call_function(function.clone(), vec![item], span)?);
                }
                Ok(Value::List(out))
            }
            BuiltinId::Filter => {
                let [list, callback] = expect_arity(args, span)?;
                let Value::List(items) = list else {
                    return Err(runtime_error(
                        span,
                        "`filter` expects a list as first argument",
                    ));
                };
                let Value::Function(function) = callback else {
                    return Err(runtime_error(
                        span,
                        "`filter` expects a function as second argument",
                    ));
                };

                let mut out = Vec::new();
                for item in items {
                    let keep = self.call_function(function.clone(), vec![item.clone()], span)?;
                    if is_truthy(&keep) {
                        out.push(item);
                    }
                }
                Ok(Value::List(out))
            }
        }
    }

    fn ensure_builtin_arity(
        &self,
        spec: &BuiltinSpec,
        actual: usize,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let variadic = spec.args.last().map(|arg| arg.variadic).unwrap_or(false);
        let min = spec.args.iter().filter(|arg| !arg.optional).count();
        let max = if variadic {
            usize::MAX
        } else {
            spec.args.len()
        };
        if actual < min || actual > max {
            let max_text = if variadic {
                "many".to_string()
            } else {
                max.to_string()
            };
            return Err(runtime_error(
                span,
                format!(
                    "{} expects between {} and {} argument(s), got {}",
                    format_usage(spec),
                    min,
                    max_text,
                    actual
                ),
            ));
        }
        Ok(())
    }

    fn expect_env_name(&self, value: Value, span: Span) -> Result<String, Diagnostic> {
        let name = stringify_value(&value);
        if is_valid_env_name(&name) {
            Ok(name)
        } else {
            Err(runtime_error(
                span,
                format!("invalid environment variable name `{name}`"),
            ))
        }
    }

    fn run_external_command(
        &mut self,
        command_name: &str,
        args: Vec<Value>,
        stdin_value: Option<Value>,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let mut command = Command::new(command_name);
        command.current_dir(&self.shell.cwd);
        command.env_clear();
        command.envs(self.shell.env.clone());
        command.args(args.iter().map(stringify_value));

        if self.shell.interactive && stdin_value.is_none() {
            INTERRUPTED.store(false, Ordering::SeqCst);
            command.stdin(Stdio::inherit());
            command.stdout(Stdio::inherit());
            command.stderr(Stdio::inherit());
            let status = command.status().map_err(|err| {
                runtime_error(span, format!("failed to run `{command_name}`: {err}"))
            })?;

            if take_interrupt() {
                return Ok(Value::Null);
            }

            if !status.success() {
                return Err(runtime_error(
                    span,
                    format!("command `{command_name}` exited with status {}", status),
                ));
            }

            return Ok(Value::Null);
        }

        command.stdout(Stdio::piped());
        command.stderr(Stdio::inherit());

        let output = if let Some(input) = stdin_value {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().map_err(|err| {
                runtime_error(span, format!("failed to run `{command_name}`: {err}"))
            })?;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin
                    .write_all(stringify_value(&input).as_bytes())
                    .map_err(|err| runtime_error(span, format!("failed to write stdin: {err}")))?;
            }
            child
                .wait_with_output()
                .map_err(|err| runtime_error(span, format!("failed to wait for process: {err}")))?
        } else {
            command.output().map_err(|err| {
                runtime_error(span, format!("failed to run `{command_name}`: {err}"))
            })?
        };

        if !output.status.success() {
            return Err(runtime_error(
                span,
                format!(
                    "command `{command_name}` exited with status {}",
                    output.status
                ),
            ));
        }

        Ok(Value::String(
            String::from_utf8_lossy(&output.stdout).to_string(),
        ))
    }

    fn eval_unary(&self, op: UnaryOp, value: Value, span: Span) -> Result<Value, Diagnostic> {
        match (op, value) {
            (UnaryOp::Negate, Value::Int(value)) => Ok(Value::Int(-value)),
            (UnaryOp::Negate, Value::Float(value)) => Ok(Value::Float(-value)),
            (UnaryOp::Not, value) => Ok(Value::Bool(!is_truthy(&value))),
            (UnaryOp::BitNot, Value::Int(value)) => Ok(Value::Int(!value)),
            _ => Err(runtime_error(span, "invalid unary operation")),
        }
    }

    fn eval_binary(
        &self,
        op: BinaryOp,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        match op {
            BinaryOp::Add => add_values(left, right, span),
            BinaryOp::Subtract => numeric_binary(left, right, span, |a, b| a - b, |a, b| a - b),
            BinaryOp::Multiply => numeric_binary(left, right, span, |a, b| a * b, |a, b| a * b),
            BinaryOp::Divide => numeric_binary(left, right, span, |a, b| a / b, |a, b| a / b),
            BinaryOp::Modulo => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
                _ => Err(runtime_error(span, "invalid `%` operands")),
            },
            BinaryOp::Power => {
                numeric_binary(left, right, span, |a, b| a.pow(b as u32), |a, b| a.powf(b))
            }
            BinaryOp::ShiftLeft => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a << b)),
                _ => Err(runtime_error(span, "invalid `<<` operands")),
            },
            BinaryOp::ShiftRight => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a >> b)),
                _ => Err(runtime_error(span, "invalid `>>` operands")),
            },
            BinaryOp::Less => compare_values(left, right, span, |ord| ord.is_lt()),
            BinaryOp::LessEqual => compare_values(left, right, span, |ord| ord.is_le()),
            BinaryOp::Greater => compare_values(left, right, span, |ord| ord.is_gt()),
            BinaryOp::GreaterEqual => compare_values(left, right, span, |ord| ord.is_ge()),
            BinaryOp::Equal => Ok(Value::Bool(values_equal(&left, &right))),
            BinaryOp::NotEqual => Ok(Value::Bool(!values_equal(&left, &right))),
            BinaryOp::BitAnd => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
                _ => Err(runtime_error(span, "invalid `&` operands")),
            },
            BinaryOp::BitXor => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
                _ => Err(runtime_error(span, "invalid `^` operands")),
            },
            BinaryOp::BitOr => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
                _ => Err(runtime_error(span, "invalid `|` operands")),
            },
            BinaryOp::LogicalAnd => Ok(Value::Bool(is_truthy(&left) && is_truthy(&right))),
            BinaryOp::LogicalOr => Ok(Value::Bool(is_truthy(&left) || is_truthy(&right))),
        }
    }
}

impl Environment {
    fn new(parent: Option<EnvRef>) -> EnvRef {
        Rc::new(Self {
            parent,
            bindings: RefCell::new(HashMap::new()),
        })
    }

    fn define(&self, name: String, value: Value) {
        self.bindings
            .borrow_mut()
            .insert(name, Rc::new(RefCell::new(value)));
    }

    fn resolve(&self, name: &str) -> Option<Rc<RefCell<Value>>> {
        if let Some(value) = self.bindings.borrow().get(name) {
            return Some(value.clone());
        }
        self.parent.as_ref().and_then(|parent| parent.resolve(name))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Int(value) => write!(f, "{value}"),
            Value::Float(value) => write!(f, "{value}"),
            Value::String(value) => write!(f, "{value}"),
            Value::List(values) => {
                write!(f, "[")?;
                for (idx, value) in values.iter().enumerate() {
                    if idx > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{value}")?;
                }
                write!(f, "]")
            }
            Value::Object(values) => {
                write!(f, "{{")?;
                for (idx, (key, value)) in values.iter().enumerate() {
                    if idx > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{key}: {value}")?;
                }
                write!(f, "}}")
            }
            Value::Function(_) => write!(f, "<function>"),
        }
    }
}

fn expect_arity<const N: usize>(args: Vec<Value>, span: Span) -> Result<[Value; N], Diagnostic> {
    args.try_into().map_err(|args: Vec<Value>| {
        runtime_error(
            span,
            format!("expected {N} argument(s), got {}", args.len()),
        )
    })
}

fn stringify_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Int(value) => *value != 0,
        Value::Float(value) => *value != 0.0,
        Value::String(value) => !value.is_empty(),
        Value::List(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        Value::Function(_) => true,
    }
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        _ => false,
    }
}

fn add_values(left: Value, right: Value, span: Span) -> Result<Value, Diagnostic> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
        (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
        (Value::String(a), b) => Ok(Value::String(a + &stringify_value(&b))),
        (a, Value::String(b)) => Ok(Value::String(stringify_value(&a) + &b)),
        _ => Err(runtime_error(span, "invalid `+` operands")),
    }
}

fn numeric_binary<FInt, FFloat>(
    left: Value,
    right: Value,
    span: Span,
    int_op: FInt,
    float_op: FFloat,
) -> Result<Value, Diagnostic>
where
    FInt: Fn(i64, i64) -> i64,
    FFloat: Fn(f64, f64) -> f64,
{
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(a, b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a, b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(a as f64, b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(a, b as f64))),
        _ => Err(runtime_error(span, "invalid numeric operands")),
    }
}

fn compare_values<F>(
    left: Value,
    right: Value,
    span: Span,
    predicate: F,
) -> Result<Value, Diagnostic>
where
    F: Fn(std::cmp::Ordering) -> bool,
{
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(predicate(a.cmp(&b)))),
        (Value::Float(a), Value::Float(b)) => a
            .partial_cmp(&b)
            .map(|ord| Value::Bool(predicate(ord)))
            .ok_or_else(|| runtime_error(span, "invalid float comparison")),
        (Value::String(a), Value::String(b)) => Ok(Value::Bool(predicate(a.cmp(&b)))),
        _ => Err(runtime_error(span, "invalid comparison operands")),
    }
}

fn runtime_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::RuntimeError, span, message)
}

#[cfg(test)]
mod tests {
    use super::{Session, ShellContext, Value, run_file, run_file_in_session};
    use pulzar_parser::parse_file;
    use pulzar_sema::analyze_file;
    use pulzar_syntax::SourceId;
    use std::path::PathBuf;

    fn run(source: &str) -> (Option<Value>, Vec<pulzar_syntax::Diagnostic>) {
        let parsed = parse_file(source, SourceId(0));
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let sema = analyze_file(&parsed.file);
        assert!(sema.diagnostics.is_empty(), "{:?}", sema.diagnostics);
        let mut shell = ShellContext::default();
        shell.cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root should exist");
        let result = run_file(&parsed.file, &mut shell);
        (result.value, result.diagnostics)
    }

    #[test]
    fn evaluates_let_and_assign() {
        let (value, diags) = run("let x = 1\n$x = 2\n$x");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::Int(2))));
    }

    #[test]
    fn evaluates_closure_with_outer_binding() {
        let (value, diags) = run("let x = 1\nlet f = y => $x + $y\n$x = 2\n$f 1");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::Int(3))));
    }

    #[test]
    fn evaluates_map_and_filter() {
        let (value, diags) = run("map([1, 2, 3], x => $x * 2)");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::List(values)) if values.len() == 3));

        let (value, diags) = run("filter([1, 2, 3], x => $x > 1)");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::List(values)) if values.len() == 2));
    }

    #[test]
    fn evaluates_member_access() {
        let (value, diags) = run("let user = @{name: 'a'}\n$user.name");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::String(value)) if value == "a"));
    }

    #[test]
    fn treats_bareword_arguments_as_strings() {
        let (value, diags) = run("cat LICENSE");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::String(value)) if !value.is_empty()));
    }

    #[test]
    fn reports_unknown_command() {
        let (value, diags) = run("command_that_should_not_exist_hopefully");
        assert!(value.is_none() || !diags.is_empty());
    }

    #[test]
    fn reads_and_writes_env_vars() {
        let (value, diags) = run("$$PULZAR_TEST = '123'\n$$PULZAR_TEST");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::String(value)) if value == "123"));
    }

    #[test]
    fn help_and_subcommand_aliases_work() {
        let (value, diags) = run("help fs read");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::String(value)) if value.contains("fs read")));

        let (value, diags) = run("fs pwd");
        assert!(diags.is_empty(), "{:?}", diags);
        assert!(matches!(value, Some(Value::String(value)) if !value.is_empty()));
    }

    #[test]
    fn session_preserves_bindings_between_runs() {
        let mut session = Session::new(ShellContext::default());
        session.shell_mut().cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root should exist");

        let first = parse_file("let x = 1", SourceId(0));
        assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
        let first_sema = analyze_file(&first.file);
        assert!(
            first_sema.diagnostics.is_empty(),
            "{:?}",
            first_sema.diagnostics
        );
        let first_runtime = run_file_in_session(&first.file, &mut session);
        assert!(
            first_runtime.diagnostics.is_empty(),
            "{:?}",
            first_runtime.diagnostics
        );

        let second = parse_file("$x", SourceId(0));
        assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
        let second_sema = analyze_file(&second.file);
        assert!(
            second_sema.diagnostics.is_empty(),
            "{:?}",
            second_sema.diagnostics
        );
        let second_runtime = run_file_in_session(&second.file, &mut session);
        assert!(
            second_runtime.diagnostics.is_empty(),
            "{:?}",
            second_runtime.diagnostics
        );
        assert!(matches!(second_runtime.value, Some(Value::Int(1))));
    }

    #[test]
    fn session_stateful_builtins_mutate_shell() {
        let mut session = Session::new(ShellContext::default());
        session.shell_mut().cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root should exist");

        let env_set = parse_file("env set PULZAR_RT abc", SourceId(0));
        assert!(env_set.diagnostics.is_empty(), "{:?}", env_set.diagnostics);
        let env_set_sema = analyze_file(&env_set.file);
        assert!(
            env_set_sema.diagnostics.is_empty(),
            "{:?}",
            env_set_sema.diagnostics
        );
        let env_set_result = run_file_in_session(&env_set.file, &mut session);
        assert!(
            env_set_result.diagnostics.is_empty(),
            "{:?}",
            env_set_result.diagnostics
        );

        let env_read = parse_file("$$PULZAR_RT", SourceId(0));
        assert!(
            env_read.diagnostics.is_empty(),
            "{:?}",
            env_read.diagnostics
        );
        let env_read_sema = analyze_file(&env_read.file);
        assert!(
            env_read_sema.diagnostics.is_empty(),
            "{:?}",
            env_read_sema.diagnostics
        );
        let env_read_result = run_file_in_session(&env_read.file, &mut session);
        assert!(
            env_read_result.diagnostics.is_empty(),
            "{:?}",
            env_read_result.diagnostics
        );
        assert!(matches!(env_read_result.value, Some(Value::String(value)) if value == "abc"));

        let cd = parse_file("cd crates", SourceId(0));
        assert!(cd.diagnostics.is_empty(), "{:?}", cd.diagnostics);
        let cd_sema = analyze_file(&cd.file);
        assert!(cd_sema.diagnostics.is_empty(), "{:?}", cd_sema.diagnostics);
        let cd_result = run_file_in_session(&cd.file, &mut session);
        assert!(
            cd_result.diagnostics.is_empty(),
            "{:?}",
            cd_result.diagnostics
        );
        assert!(session.cwd().ends_with("crates"));
    }
}
