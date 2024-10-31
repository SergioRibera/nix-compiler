use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use rnix::ast::{self, AstToken, HasEntry};
use rowan::ast::AstNode;

use crate::result::NixSpan;
use crate::value::{NixLambda, NixList};
use crate::{
    AsAttrSet, AsString, LazyNixValue, NixError, NixLabel, NixLabelKind, NixLabelMessage,
    NixLambdaParam, NixResult, NixValue, NixValueWrapped, Scope,
};

#[allow(unused_variables, reason = "todo")]
impl Scope {
    fn insert_to_attrset(
        self: &Rc<Self>,
        out: NixValueWrapped,
        attrpath: ast::Attrpath,
        attr_value: ast::Expr,
    ) -> NixResult {
        let mut attr_path: Vec<ast::Attr> = attrpath.attrs().collect();
        let last_attr_path = attr_path
            .pop()
            .expect("Attrpath requires at least one attribute");

        let target = self.resolve_attr_set_path(out.clone(), attr_path.into_iter())?;

        if !target.borrow().is_attr_set() {
            todo!("Error handling")
        };

        let attr = self.resolve_attr(&last_attr_path)?;

        // if attr.starts_with("crossSystem") {
        //     println!(
        //         "{}",
        //         NixError {
        //             labels: vec![
        //                 NixLabel::from_syntax_node(
        //                     &self.file,
        //                     attrpath.syntax(),
        //                     NixLabelMessage::Custom(attr.clone()),
        //                     NixLabelKind::Help
        //                 ),
        //                 NixLabel::from_syntax_node(
        //                     &self.file,
        //                     attr_value.syntax(),
        //                     NixLabelMessage::Custom(attr.clone()),
        //                     NixLabelKind::Help
        //                 ),
        //             ],
        //             message: format!("Tracing insert_to_attrset: {:#?}", self)
        //         }
        //     );
        // }

        let child = LazyNixValue::Pending(self.clone().new_child(), attr_value).wrap_var();

        let mut target = target.borrow_mut();
        let set = target.as_attr_set_mut().unwrap();

        set.insert(attr, child);

        Ok(out)
    }

    fn insert_entry_to_attrset(
        self: &Rc<Self>,
        out: NixValueWrapped,
        entry: ast::Entry,
    ) -> NixResult {
        match entry {
            ast::Entry::Inherit(entry) => {
                let from = entry.from().map(|from| {
                    (
                        LazyNixValue::Pending(self.clone(), from.expr().unwrap()).wrap_var(),
                        from,
                    )
                });

                for attr_node in entry.attrs() {
                    let attr = self.resolve_attr(&attr_node)?;

                    if let Some((from, from_expr)) = &from {
                        let value = {
                            let from = from.clone();
                            let from_expr = from_expr.clone();
                            let attr = attr.clone();
                            let attr_node = attr_node.clone();
                            let file = self.file.clone();

                            LazyNixValue::new_eval(
                                self.clone(),
                                from_expr.syntax().clone(),
                                Box::new(move |scope| {
                                    from.resolve()?
                                        .borrow()
                                        .as_attr_set()
                                        .unwrap()
                                        .get(&attr)
                                        .cloned()
                                        .ok_or_else(|| NixError {
                                            message: format!(
                                                "Attribute '\x1b[1;95m{attr}\x1b[0m' missing"
                                            ),
                                            labels: vec![
                                                NixLabel::new(
                                                    NixSpan::from_ast_node(&file, &attr_node)
                                                        .into(),
                                                    NixLabelMessage::AttributeMissing,
                                                    NixLabelKind::Error,
                                                ),
                                                NixLabel::new(
                                                    NixSpan::from_ast_node(&file, &from_expr)
                                                        .into(),
                                                    NixLabelMessage::Custom(
                                                        "Parent attrset".to_owned(),
                                                    ),
                                                    NixLabelKind::Help,
                                                ),
                                            ],
                                            backtrace: None,
                                        })?
                                        .resolve()
                                }),
                            )
                        };

                        out.borrow_mut()
                            .as_attr_set_mut()
                            .unwrap()
                            .insert(attr, value.wrap_var());
                    } else {
                        let value = {
                            let attr = attr.clone();
                            let attr_node = attr_node.clone();
                            let file = self.file.clone();

                            LazyNixValue::new_eval(
                                self.clone(),
                                attr_node.syntax().clone(),
                                Box::new(move |scope| {
                                    let Some(value) = scope.get_variable(attr.clone()) else {
                                        return Err(NixError::from_message(
                                            NixLabel::new(
                                                NixSpan::from_ast_node(&file, &attr_node).into(),
                                                NixLabelMessage::VariableNotFound,
                                                NixLabelKind::Error,
                                            ),
                                            format!("Variable '{attr} not found"),
                                        ));
                                    };

                                    value.resolve()
                                }),
                            )
                        };

                        out.borrow_mut()
                            .as_attr_set_mut()
                            .unwrap()
                            .insert(attr, value.wrap_var());
                    }
                }

                Ok(out)
            }
            ast::Entry::AttrpathValue(entry) => {
                self.insert_to_attrset(out, entry.attrpath().unwrap(), entry.value().unwrap())
            }
        }
    }

    pub fn visit_expr(self: &Rc<Self>, node: ast::Expr) -> NixResult {
        match node {
            ast::Expr::Apply(node) => self.visit_apply(node),
            ast::Expr::Assert(node) => self.visit_assert(node),
            ast::Expr::AttrSet(node) => self.visit_attrset(node),
            ast::Expr::BinOp(node) => self.visit_binop(node),
            ast::Expr::Error(node) => self.visit_error(node),
            ast::Expr::HasAttr(node) => self.visit_hasattr(node),
            ast::Expr::Ident(node) => self.visit_ident(node),
            ast::Expr::IfElse(node) => self.visit_ifelse(node),
            ast::Expr::Lambda(node) => self.visit_lambda(node),
            ast::Expr::LegacyLet(node) => self.visit_legacylet(node),
            ast::Expr::LetIn(node) => self.visit_letin(node),
            ast::Expr::List(node) => self.visit_list(node),
            ast::Expr::Literal(node) => self.visit_literal(node),
            ast::Expr::Paren(node) => self.visit_paren(node),
            ast::Expr::Path(node) => self.visit_path(node),
            ast::Expr::Root(node) => self.visit_root(node),
            ast::Expr::Select(node) => self.visit_select(node),
            ast::Expr::Str(node) => self.visit_str(node),
            ast::Expr::UnaryOp(node) => self.visit_unaryop(node),
            ast::Expr::With(node) => self.visit_with(node),
        }
    }

    pub fn visit_apply(self: &Rc<Self>, node: ast::Apply) -> NixResult {
        let lambda = self.visit_expr(node.lambda().unwrap())?;

        let lambda = lambda.borrow();

        match lambda.deref() {
            NixValue::Builtin(builtin) => builtin.run(self.clone(), node.argument().unwrap()),
            NixValue::Lambda(NixLambda(scope, param, expr)) => {
                let scope = scope.clone().new_child();

                match param {
                    NixLambdaParam::Pattern(pattern) => {
                        let argument_var = self.visit_expr(node.argument().unwrap())?;
                        let argument = argument_var.borrow();
                        let Some(argument) = argument.as_attr_set() else {
                            todo!("Error handling")
                        };

                        if let Some(pat_bind) = pattern.pat_bind() {
                            let varname = pat_bind
                                .ident()
                                .unwrap()
                                .ident_token()
                                .unwrap()
                                .text()
                                .to_owned();

                            // TODO: Should set only the unused keys instead of the argument
                            scope.set_variable(
                                varname,
                                LazyNixValue::Concrete(argument_var.clone()).wrap_var(),
                            );
                        }

                        let has_ellipsis = pattern.ellipsis_token().is_some();

                        let mut unused =
                            (!has_ellipsis).then(|| argument.keys().collect::<Vec<_>>());

                        for entry in pattern.pat_entries() {
                            let varname = entry.ident().unwrap().ident_token().unwrap();
                            let varname = varname.text();

                            if let Some(unused) = unused.as_mut() {
                                unused.swap_remove(
                                    unused.iter().position(|&key| key == varname).expect("Hola"),
                                );
                            }

                            // if varname.starts_with("crossSystem") {
                            //     println!(
                            //         "{}",
                            //         NixError {
                            //             labels: vec![NixLabel::from_syntax_node(
                            //                 &scope.file,
                            //                 entry.ident().unwrap().syntax(),
                            //                 NixLabelMessage::Custom(varname.to_owned()),
                            //                 NixLabelKind::Help
                            //             ),],
                            //             message: format!("Tracing pattern params")
                            //         }
                            //     );
                            // }

                            let var = if let Some(var) = argument.get(varname).cloned() {
                                var
                            } else {
                                if let Some(expr) = entry.default() {
                                    LazyNixValue::Concrete(scope.visit_expr(expr)?).wrap_var()
                                } else {
                                    todo!("Require {varname}");
                                }
                            };

                            scope.set_variable(varname.to_owned(), var.clone());
                        }

                        if let Some(unused) = unused {
                            if !unused.is_empty() {
                                todo!("Handle error: Unused keys: {unused:?}")
                            }
                        }
                    }
                    NixLambdaParam::Ident(param) => {
                        assert!(
                            scope
                                .set_variable(
                                    param.clone(),
                                    LazyNixValue::Pending(self.clone(), node.argument().unwrap())
                                        .wrap_var()
                                )
                                .is_none(),
                            "Variable {param} already exists"
                        );
                    }
                }

                scope.visit_expr(expr.clone())
            }

            a => todo!("Error handling: {a:#?}"),
        }
    }

    pub fn visit_assert(self: &Rc<Self>, node: ast::Assert) -> NixResult {
        let condition = self.visit_expr(node.condition().unwrap())?;
        let Some(condition) = condition.borrow().as_bool() else {
            todo!("Error handling")
        };

        if condition {
            node.body()
                .map_or_else(|| Ok(NixValue::Null.wrap()), |expr| self.visit_expr(expr))
        } else {
            Err(NixError::from_message(
                NixLabel::new(
                    NixSpan::from_ast_node(&self.file, &node.condition().unwrap()).into(),
                    NixLabelMessage::AssertionFailed,
                    NixLabelKind::Error,
                ),
                "assert failed",
            ))
        }
    }

    pub fn visit_attrset(self: &Rc<Self>, node: ast::AttrSet) -> NixResult {
        let is_recursive = node.rec_token().is_some();

        if is_recursive {
            let scope = self.clone().new_child();

            for entry in node.entries() {
                scope.insert_entry_to_attrset(scope.variables.clone(), entry)?;
            }

            Ok(scope.variables.clone())
        } else {
            let out = NixValue::AttrSet(HashMap::new()).wrap();

            for entry in node.entries() {
                self.insert_entry_to_attrset(out.clone(), entry)?;
            }

            Ok(out)
        }
    }

    pub fn visit_binop(self: &Rc<Self>, node: ast::BinOp) -> NixResult {
        let lhs = self.visit_expr(node.lhs().unwrap())?;

        match node.operator().unwrap() {
            ast::BinOpKind::Concat => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Concat op",
                None,
            )),
            ast::BinOpKind::Update => lhs
                .borrow()
                .as_attr_set()
                .cloned()
                .ok_or_else(|| todo!("Error handling"))
                .and_then(|mut lhs| {
                    self.visit_expr(node.rhs().unwrap()).and_then(|rhs| {
                        rhs.borrow()
                            .as_attr_set()
                            .ok_or_else(|| todo!("Error handling"))
                            .map(|rhs| {
                                rhs.into_iter().for_each(|(key, value)| {
                                    lhs.insert(key.clone(), value.clone());
                                });
                            })
                            .map(|_| NixValue::AttrSet(lhs).wrap())
                    })
                }),
            ast::BinOpKind::Add => match lhs.borrow().deref() {
                NixValue::String(lhs) => self
                    .visit_expr(node.rhs().unwrap())?
                    .borrow()
                    .as_string()
                    .ok_or_else(|| todo!("Error handling"))
                    .map(|rhs| NixValue::String(format!("{lhs}{rhs}")).wrap()),
                _ => Err(NixError::todo(
                    NixSpan::from_ast_node(&self.file, &node).into(),
                    "Cannot add",
                    None,
                )),
            },
            ast::BinOpKind::Sub => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Sub op",
                None,
            )),
            ast::BinOpKind::Mul => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Mul op",
                None,
            )),
            ast::BinOpKind::Div => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Div op",
                None,
            )),
            ast::BinOpKind::And => lhs
                .borrow()
                .as_bool()
                .ok_or_else(|| todo!("Error handling"))
                .and_then(|lhs| {
                    lhs.then(|| self.visit_expr(node.rhs().unwrap()))
                        .unwrap_or_else(|| Ok(NixValue::Bool(false).wrap()))
                }),
            ast::BinOpKind::Equal => self
                .visit_expr(node.rhs().unwrap())
                .map(|rhs| rhs.borrow().deref().eq(&lhs.borrow()))
                .map(NixValue::Bool)
                .map(NixValue::wrap),
            ast::BinOpKind::Implication => lhs
                .borrow()
                .as_bool()
                .ok_or_else(|| todo!("Error handling"))
                .and_then(|lhs| {
                    lhs.then(|| self.visit_expr(node.rhs().unwrap()))
                        .unwrap_or_else(|| Ok(NixValue::Bool(true).wrap()))
                }),
            ast::BinOpKind::Less => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Less op",
                None,
            )),
            ast::BinOpKind::LessOrEq => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "LessOrEq op",
                None,
            )),
            ast::BinOpKind::More => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "More op",
                None,
            )),
            ast::BinOpKind::MoreOrEq => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "MoreOrEq op",
                None,
            )),
            ast::BinOpKind::NotEqual => self
                .visit_expr(node.rhs().unwrap())
                .map(|rhs| rhs.borrow().deref().ne(&lhs.borrow()))
                .map(NixValue::Bool)
                .map(NixValue::wrap),
            ast::BinOpKind::Or => lhs
                .borrow()
                .as_bool()
                .ok_or_else(|| todo!("Error handling"))
                .and_then(|lhs| {
                    (!lhs)
                        .then(|| self.visit_expr(node.rhs().unwrap()))
                        .unwrap_or_else(|| Ok(NixValue::Bool(true).wrap()))
                }),
        }
    }

    pub fn visit_error(self: &Rc<Self>, node: ast::Error) -> NixResult {
        Err(NixError::todo(
            NixSpan::from_ast_node(&self.file, &node).into(),
            "Error Expr",
            None,
        ))
    }

    pub fn visit_hasattr(self: &Rc<Self>, node: ast::HasAttr) -> NixResult {
        let value = self.visit_expr(node.expr().unwrap())?;

        let has_attr = self
            .resolve_attr_path(value, node.attrpath().unwrap())
            // TODO: Check VariableNotFound error
            .is_ok();

        Ok(NixValue::Bool(has_attr).wrap())
    }

    pub fn visit_ident(self: &Rc<Self>, node: ast::Ident) -> NixResult {
        let node = node.ident_token().unwrap();
        let varname = node.text().to_string();

        // if varname.starts_with("crossSystem") {
        //     println!(
        //         "{}",
        //         NixError::from_message(
        //             NixLabel::new(
        //                 &self.file,
        //                 &node,
        //                 NixLabelMessage::Custom(varname.clone()),
        //                 NixLabelKind::Help
        //             ),
        //             "Tracing visit_ident"
        //         )
        //     );
        // }

        self.get_variable(varname.clone())
            .ok_or_else(|| {
                NixError::from_message(
                    NixLabel::new(
                        NixSpan::from_syntax_token(&self.file, &node).into(),
                        NixLabelMessage::VariableNotFound,
                        NixLabelKind::Error,
                    ),
                    format!("Variable '\x1b[1;95m{varname}\x1b[0m' not found"),
                )
            })?
            .resolve()
    }

    pub fn visit_ifelse(self: &Rc<Self>, node: ast::IfElse) -> NixResult {
        let condition = self.visit_expr(node.condition().unwrap())?;
        let Some(condition) = condition.borrow().as_bool() else {
            todo!("Error handling")
        };

        if condition {
            self.visit_expr(node.body().unwrap())
        } else {
            self.visit_expr(node.else_body().unwrap())
        }
    }

    pub fn visit_lambda(self: &Rc<Self>, node: ast::Lambda) -> NixResult {
        let param = match node.param().unwrap() {
            ast::Param::Pattern(pattern) => NixLambdaParam::Pattern(pattern),
            ast::Param::IdentParam(ident) => NixLambdaParam::Ident(
                ident
                    .ident()
                    .unwrap()
                    .ident_token()
                    .unwrap()
                    .text()
                    .to_owned(),
            ),
        };

        Ok(NixValue::Lambda(NixLambda(
            self.clone().new_child(),
            param,
            node.body().unwrap(),
        ))
        .wrap())
    }

    pub fn visit_legacylet(self: &Rc<Self>, node: ast::LegacyLet) -> NixResult {
        unimplemented!("This is legacy")
    }

    pub fn visit_letin(self: &Rc<Self>, node: ast::LetIn) -> NixResult {
        for entry in node.entries() {
            self.insert_entry_to_attrset(self.variables.clone(), entry)?;
        }

        let body = node.body().unwrap();

        self.visit_expr(body)
    }

    pub fn visit_list(self: &Rc<Self>, node: ast::List) -> NixResult {
        Ok(NixValue::List(NixList(Rc::new(
            node.items()
                .map(|expr| LazyNixValue::Pending(self.clone(), expr).wrap_var())
                .collect(),
        )))
        .wrap())
    }

    pub fn visit_literal(self: &Rc<Self>, node: ast::Literal) -> NixResult {
        match node.kind() {
            ast::LiteralKind::Float(_) => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Float literal",
                None,
            )),
            ast::LiteralKind::Integer(value) => Ok(NixValue::Int(value.value().unwrap()).wrap()),
            ast::LiteralKind::Uri(_) => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Uri literal",
                None,
            )),
        }
    }

    pub fn visit_paren(self: &Rc<Self>, node: ast::Paren) -> NixResult {
        self.visit_expr(node.expr().unwrap())
    }

    pub fn visit_path(self: &Rc<Self>, node: ast::Path) -> NixResult {
        let mut path = String::new();

        for (idx, part) in node.parts().enumerate() {
            match part {
                ast::InterpolPart::Literal(str) => {
                    let str = str.syntax().text();

                    if idx == 0 {
                        if &str[0..1] == "/" {
                            path += str;
                        } else {
                            let dirname = self.file.path.parent().expect("Cannot get parent");

                            if str.get(1..2) == Some(".") {
                                let Some(parent) = dirname.parent() else {
                                    return Err(NixError::todo(
                                        NixSpan::from_ast_node(&self.file, &node).into(),
                                        "Error handling: path doesn't have parent",
                                        None,
                                    ));
                                };
                                path += &parent.display().to_string();
                                path += &str[2..];
                            } else {
                                path += &dirname.display().to_string();
                                path += &str[1..];
                            }
                        }
                    } else {
                        if path.chars().rev().next() != Some('/') {
                            path += "/";
                        }

                        path += str;
                    }
                }
                ast::InterpolPart::Interpolation(interpol) => {
                    let str = self
                        .visit_expr(interpol.expr().unwrap())?
                        .borrow()
                        .as_string()
                        .unwrap();

                    if idx == 1 && path.get(0..1) == Some("/") && str.get(0..1) == Some("/") {
                        path.pop();
                    }

                    path += &str;
                }
            }
        }

        Ok(NixValue::Path(path.try_into().expect("TODO: Error handling")).wrap())
    }

    pub fn visit_root(self: &Rc<Self>, node: ast::Root) -> NixResult {
        self.visit_expr(node.expr().unwrap())
    }

    pub fn visit_select(self: &Rc<Self>, node: ast::Select) -> NixResult {
        let var = self.visit_expr(node.expr().unwrap())?;

        self.resolve_attr_path(var, node.attrpath().unwrap())?
            .resolve()
    }

    pub fn visit_str(self: &Rc<Self>, node: ast::Str) -> NixResult {
        let mut content = String::new();

        for part in node.parts() {
            match part {
                ast::InterpolPart::Literal(str) => {
                    content += str.syntax().text();
                }
                ast::InterpolPart::Interpolation(interpol) => {
                    content += &self
                        .visit_expr(interpol.expr().unwrap())?
                        .borrow()
                        .as_string()
                        .unwrap();
                }
            }
        }

        Ok(NixValue::String(content).wrap())
    }

    pub fn visit_unaryop(self: &Rc<Self>, node: ast::UnaryOp) -> NixResult {
        let value = self.visit_expr(node.expr().unwrap())?;
        let value = value.borrow();

        match node.operator().unwrap() {
            ast::UnaryOpKind::Invert => {
                let Some(value) = value.as_bool() else {
                    todo!("Error handling");
                };

                Ok(NixValue::Bool(!value).wrap())
            }
            ast::UnaryOpKind::Negate => Err(NixError::todo(
                NixSpan::from_ast_node(&self.file, &node).into(),
                "Negate op",
                None,
            )),
        }
    }

    pub fn visit_with(self: &Rc<Self>, node: ast::With) -> NixResult {
        let namespace = self.visit_expr(node.namespace().unwrap())?;

        if !namespace.borrow().is_attr_set() {
            todo!("Error handling")
        }

        let scope = self.clone().new_child_from(namespace);

        scope.visit_expr(node.body().unwrap())
    }
}
