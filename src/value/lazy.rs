use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::{fmt, mem};

use rnix::ast;

use crate::{
    AsAttrSet, NixBacktrace, NixError, NixLabel, NixLabelKind, NixLabelMessage, NixResult, NixSpan,
    NixValueWrapped, NixVar, Scope,
};

use super::NixLambda;

#[derive(Clone)]
pub enum LazyNixValue {
    Concrete(NixValueWrapped),
    Pending(Rc<NixBacktrace>, Rc<Scope>, ast::Expr),
    Eval(
        Rc<NixBacktrace>,
        Rc<Scope>,
        Rc<RefCell<Option<Box<dyn FnOnce(Rc<NixBacktrace>, Rc<Scope>) -> NixResult>>>>,
    ),
    Resolving(Rc<NixBacktrace>),
}

impl fmt::Debug for LazyNixValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LazyNixValue::Concrete(value) => fmt::Debug::fmt(value.borrow().deref(), f),
            LazyNixValue::Pending(..) => f.write_str("<not-resolved>"),
            LazyNixValue::Eval(..) => f.write_str("<not-resolved>"),
            LazyNixValue::Resolving(..) => f.write_str("<resolving>"),
        }
    }
}

impl fmt::Display for LazyNixValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LazyNixValue::Concrete(value) => fmt::Display::fmt(value.borrow().deref(), f),
            LazyNixValue::Pending(..) => f.write_str("<not-resolved>"),
            LazyNixValue::Eval(..) => f.write_str("<not-resolved>"),
            LazyNixValue::Resolving(..) => f.write_str("<resolving>"),
        }
    }
}

impl LazyNixValue {
    pub fn try_eq(
        lhs: &Rc<RefCell<Self>>,
        rhs: &Rc<RefCell<Self>>,
        backtrace: Rc<NixBacktrace>,
    ) -> NixResult<bool> {
        let lhs = LazyNixValue::resolve(lhs, backtrace.clone())?;
        let rhs = LazyNixValue::resolve(rhs, backtrace)?;

        let lhs = lhs.borrow();
        let rhs = rhs.borrow();

        Ok(*lhs == *rhs)
    }
}

impl LazyNixValue {
    pub fn new_eval(
        scope: Rc<Scope>,
        backtrace: Rc<NixBacktrace>,
        fun: Box<dyn FnOnce(Rc<NixBacktrace>, Rc<Scope>) -> NixResult>,
    ) -> Self {
        LazyNixValue::Eval(backtrace, scope, Rc::new(RefCell::new(Option::Some(fun))))
    }

    pub fn new_callback_eval(
        backtrace: Rc<NixBacktrace>,
        callback: NixLambda,
        value: NixVar,
    ) -> Self {
        let NixLambda(scope, param, expr) = callback.clone();
        let span = Rc::new(NixSpan::from_ast_node(&scope.file, &expr));

        LazyNixValue::new_eval(
            scope.new_child(),
            Rc::new(NixBacktrace(span.clone(), Some(backtrace))),
            Box::new(move |backtrace, scope| {
                match param {
                    crate::NixLambdaParam::Ident(ident) => {
                        scope.set_variable(ident, value);
                    }
                    crate::NixLambdaParam::Pattern(_) => {
                        return Err(crate::NixError::todo(
                            span,
                            "Pattern lambda param",
                            Some(backtrace),
                        ))
                    }
                };

                scope.visit_expr(backtrace, expr)
            }),
        )
    }

    pub fn wrap_var(self) -> NixVar {
        NixVar(Rc::new(RefCell::new(self)))
    }

    pub fn as_concrete(&self) -> Option<NixValueWrapped> {
        if let LazyNixValue::Concrete(value) = self {
            Some(value.clone())
        } else {
            None
        }
    }

    pub fn resolve(this: &Rc<RefCell<Self>>, backtrace: Rc<NixBacktrace>) -> NixResult {
        if let Some(value) = this.borrow().as_concrete() {
            return Ok(value);
        }

        let backtrace = match *this.borrow() {
            LazyNixValue::Concrete(_) => unreachable!(),
            LazyNixValue::Pending(ref backtrace, ..) => backtrace.clone(),
            LazyNixValue::Eval(ref backtrace, ..) => backtrace.clone(),
            LazyNixValue::Resolving(ref def_backtrace) => {
                let label = NixLabelMessage::Empty;
                let kind = NixLabelKind::Error;

                let NixBacktrace(span, def_backtrace) = &**def_backtrace;

                let label = NixLabel::new(span.clone(), label, kind);
                let called_label = NixLabel::new(
                    backtrace.0.clone(),
                    NixLabelMessage::Custom("Called from here".to_string()),
                    NixLabelKind::Help,
                );

                return Err(NixError {
                    message: "Infinite recursion detected. Tried to get a value that is resolving"
                        .to_owned(),
                    labels: vec![label, called_label],
                    backtrace: def_backtrace.clone(),
                });
            }
        };

        let old = mem::replace(
            this.borrow_mut().deref_mut(),
            LazyNixValue::Resolving(backtrace.clone()),
        );

        match old {
            LazyNixValue::Concrete(..) | LazyNixValue::Resolving(..) => unreachable!(),
            LazyNixValue::Pending(_, scope, expr) => {
                let value = scope.visit_expr(backtrace, expr)?;

                *this.borrow_mut().deref_mut() = LazyNixValue::Concrete(value.clone());

                Ok(value)
            }
            LazyNixValue::Eval(_, scope, eval) => {
                let value = eval
                    .borrow_mut()
                    .take()
                    .expect("Eval cannot be called twice")(
                    backtrace, scope.clone()
                )?;

                *this.borrow_mut().deref_mut() = LazyNixValue::Concrete(value.clone());

                Ok(value)
            }
        }
    }

    pub fn resolve_set(
        this: &Rc<RefCell<Self>>,
        recursive: bool,
        backtrace: Rc<NixBacktrace>,
    ) -> NixResult {
        let value = Self::resolve(this, backtrace.clone())?;

        if value.borrow().is_attr_set() {
            let values = if let Some(set) = value.borrow().as_attr_set() {
                set.values().cloned().collect::<Vec<_>>()
            } else {
                unreachable!()
            };

            for var in values {
                if recursive {
                    var.resolve_set(true, backtrace.clone())?;
                } else {
                    var.resolve(backtrace.clone())?;
                }
            }
        } else if let Some(list) = value.borrow().as_list() {
            list.0
                .iter()
                .map(|var| {
                    if recursive {
                        var.resolve_set(true, backtrace.clone())?;
                    } else {
                        var.resolve(backtrace.clone())?;
                    }

                    Ok(())
                })
                .collect::<Result<(), _>>()?;
        }

        Ok(value)
    }
}
