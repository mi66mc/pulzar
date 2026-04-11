use std::collections::HashSet;

use pulzar_syntax::{
    Block, Diagnostic, DiagnosticKind, Expr, ExprKind, File, FnBody, LambdaBody, Param, Stmt,
    StmtKind,
};

#[derive(Debug, Clone)]
pub struct SemanticResult<'a> {
    pub file: &'a File,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn analyze_file(file: &File) -> SemanticResult<'_> {
    let mut analyzer = Analyzer::new();
    analyzer.visit_file(file);
    SemanticResult {
        file,
        diagnostics: analyzer.diagnostics,
    }
}

struct Analyzer {
    scopes: Vec<Scope>,
    diagnostics: Vec<Diagnostic>,
    return_context: usize,
}

#[derive(Debug, Default)]
struct Scope {
    bindings: HashSet<String>,
}

impl Analyzer {
    fn new() -> Self {
        Self {
            scopes: vec![Scope::default()],
            diagnostics: Vec::new(),
            return_context: 0,
        }
    }

    fn visit_file(&mut self, file: &File) {
        for statement in &file.statements {
            self.visit_stmt(statement);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let { name, value } => {
                self.visit_expr(value);
                self.declare(name, stmt.span, "duplicate binding in the same scope");
            }
            StmtKind::Assign { target, value } => {
                self.visit_expr(value);
                self.check_assignment_target(target);
            }
            StmtKind::FnDecl { name, params, body } => {
                self.declare(name, stmt.span, "duplicate binding in the same scope");
                self.push_scope();
                self.declare_params(params);
                self.enter_return_context();
                match body {
                    FnBody::Block(block) => self.visit_block(block),
                    FnBody::Expr(expr) => self.visit_expr(expr),
                }
                self.exit_return_context();
                self.pop_scope();
            }
            StmtKind::Return { value } => {
                if self.return_context == 0 {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticKind::InvalidReturnContext,
                        stmt.span,
                        "`return` is only valid inside functions or block-bodied lambdas",
                    ));
                }
                if let Some(expr) = value {
                    self.visit_expr(expr);
                }
            }
            StmtKind::Expr(expr) => self.visit_expr(expr),
        }
    }

    fn visit_block(&mut self, block: &Block) {
        self.push_scope();
        for statement in &block.statements {
            self.visit_stmt(statement);
        }
        self.pop_scope();
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Bareword(_) => {}
            ExprKind::Variable(_) => {}
            ExprKind::EnvVar(_) => {}
            ExprKind::Integer(_) => {}
            ExprKind::Float(_) => {}
            ExprKind::String(_) => {}
            ExprKind::Bool(_) => {}
            ExprKind::Error => {}
            ExprKind::Grouped(inner) => self.visit_expr(inner),
            ExprKind::List(items) => {
                for item in items {
                    self.visit_expr(item);
                }
            }
            ExprKind::Object(fields) => {
                for field in fields {
                    self.visit_expr(&field.value);
                }
            }
            ExprKind::Call { callee, args } => {
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            ExprKind::Pipeline { left, right } => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            ExprKind::Lambda { params, body } => {
                self.push_scope();
                self.declare_params(params);
                match body {
                    LambdaBody::Block(block) => {
                        self.enter_return_context();
                        self.visit_block(block);
                        self.exit_return_context();
                    }
                    LambdaBody::Expr(expr) => self.visit_expr(expr),
                }
                self.pop_scope();
            }
            ExprKind::Unary { expr, .. } => self.visit_expr(expr),
            ExprKind::Binary { left, right, .. } => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            ExprKind::Member { object, .. } => self.visit_expr(object),
        }
    }

    fn check_assignment_target(&mut self, target: &Expr) {
        match &target.kind {
            ExprKind::Variable(name) => {
                if !self.is_declared(name) {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticKind::AssignmentToUndeclaredName,
                        target.span,
                        format!("cannot assign to undeclared name `{name}`"),
                    ));
                }
            }
            ExprKind::EnvVar(_) => {}
            _ => {
                // Parser already diagnoses invalid shapes; sema only enforces binding existence.
            }
        }
    }

    fn declare_params(&mut self, params: &[Param]) {
        for param in params {
            self.declare(
                &param.name,
                param.span,
                "duplicate parameter in the same scope",
            );
        }
    }

    fn declare(&mut self, name: &str, span: pulzar_syntax::Span, message: &str) {
        let scope = self
            .scopes
            .last_mut()
            .expect("semantic analyzer must always have at least one scope");
        if !scope.bindings.insert(name.to_string()) {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::DuplicateBinding,
                span,
                message,
            ));
        }
    }

    fn is_declared(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .any(|scope| scope.bindings.contains(name))
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes
            .pop()
            .expect("semantic analyzer scope stack underflow");
    }

    fn enter_return_context(&mut self) {
        self.return_context += 1;
    }

    fn exit_return_context(&mut self) {
        self.return_context = self.return_context.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::analyze_file;
    use pulzar_parser::parse_file;
    use pulzar_syntax::{DiagnosticKind, SourceId};

    fn diagnostics(source: &str) -> Vec<DiagnosticKind> {
        let parsed = parse_file(source, SourceId(0));
        let result = analyze_file(&parsed.file);
        result
            .diagnostics
            .into_iter()
            .map(|diag| diag.kind)
            .collect()
    }

    #[test]
    fn allows_reassignment_after_let() {
        let diags = diagnostics("let x = 1\n$x = 2");
        assert!(!diags.contains(&DiagnosticKind::AssignmentToUndeclaredName));
    }

    #[test]
    fn rejects_assignment_without_binding() {
        let diags = diagnostics("$x = 2");
        assert!(diags.contains(&DiagnosticKind::AssignmentToUndeclaredName));
    }

    #[test]
    fn rejects_duplicate_let_in_same_scope() {
        let diags = diagnostics("let x = 1\nlet x = 2");
        assert!(diags.contains(&DiagnosticKind::DuplicateBinding));
    }

    #[test]
    fn allows_shadowing_in_inner_scope() {
        let diags = diagnostics("let x = 1\nfn f() { let x = 2 }");
        assert!(!diags.contains(&DiagnosticKind::DuplicateBinding));
    }

    #[test]
    fn rejects_duplicate_parameters() {
        let diags = diagnostics("fn f(x, x) { $x }");
        assert!(diags.contains(&DiagnosticKind::DuplicateBinding));
    }

    #[test]
    fn allows_return_in_function_and_block_lambda() {
        let function_diags = diagnostics("fn f() { return 1 }");
        assert!(!function_diags.contains(&DiagnosticKind::InvalidReturnContext));

        let lambda_diags = diagnostics("let f = x => { return $x }");
        assert!(!lambda_diags.contains(&DiagnosticKind::InvalidReturnContext));
    }

    #[test]
    fn rejects_return_at_top_level() {
        let diags = diagnostics("return 1");
        assert!(diags.contains(&DiagnosticKind::InvalidReturnContext));
    }

    #[test]
    fn allows_unresolved_command_like_names() {
        let diags = diagnostics("ps |> filter isAdult\nsend_email");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn supports_recursive_function_name_binding() {
        let diags = diagnostics("fn f(x) => f($x)");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn allows_env_assignment_without_local_binding() {
        let diags = diagnostics("$$PATH = 'abc'");
        assert!(!diags.contains(&DiagnosticKind::AssignmentToUndeclaredName));
    }
}
