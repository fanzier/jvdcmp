use decompiler::cfg::*;
use decompiler::types::*;
use disassembler::instructions::*;

pub fn convert_un_op(op: UnaryOp) -> UnOp {
    match op {
        UnaryOp::Neg => UnOp::Neg,
    }
}

pub fn convert_bin_op(op: BinaryOp) -> BinOp {
    match op {
        BinaryOp::Add => BinOp::Add,
        BinaryOp::Sub => BinOp::Sub,
        BinaryOp::Mul => BinOp::Mul,
        BinaryOp::Div => BinOp::Div,
        BinaryOp::Rem => BinOp::Rem,
        BinaryOp::Shl => BinOp::Shl,
        BinaryOp::Shr => BinOp::Shr,
        BinaryOp::Ushr => BinOp::Ushr,
        BinaryOp::And => BinOp::BitAnd,
        BinaryOp::Or => BinOp::BitOr,
        BinaryOp::Xor => BinOp::BitXor,
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct StackLayout(pub StackVarId);

impl StackLayout {
    pub fn new() -> Self {
        StackLayout(0)
    }

    pub fn get(&self, i: isize) -> StackVarId {
        self.0 - i
    }

    pub fn push(&mut self) -> StackVarId {
        self.0 += 1;
        self.0 - 1
    }

    pub fn pop(&mut self) -> StackVarId {
        self.0 -= 1;
        assert!(self.0 >= 0);
        self.0
    }

    pub fn execute(&mut self, instruction: &Instruction, metadata: &Metadata) -> Vec<Statement> {
        match *instruction {
            Instruction::Nop => vec![],
            Instruction::Load(ref rvalue) => {
                let expr = self.make_stack_vars_rvalue(rvalue, metadata);
                let top = self.push();
                vec![stmt_expr(Expr::Assign {
                    to: Box::new(Assignable::Variable(stack(top), 0)),
                    op: None,
                    from: Box::new(expr),
                })]
            }
            Instruction::Store(ref to) => {
                let assignable = self.make_stack_vars_lvalue(to, metadata);
                let top = self.pop();
                vec![stmt_expr(Expr::Assign {
                    to: Box::new(assignable),
                    op: None,
                    from: Box::new(mk_variable(stack(top))),
                })]
            }
            Instruction::Arithm(ref arithm) => match *arithm {
                Arithm::UnaryOp(op) => {
                    let v = self.pop();
                    let res = self.push();
                    let to = Box::new(Assignable::Variable(stack(res), 0));
                    let from = Box::new(Expr::UnaryOp(
                        convert_un_op(op),
                        Box::new(mk_variable(stack(v))),
                    ));
                    vec![stmt_expr(Expr::Assign {
                        to: to,
                        op: None,
                        from: from,
                    })]
                }
                Arithm::BinaryOp(op) => {
                    let w = self.pop();
                    let v = self.pop();
                    let res = self.push();
                    let to = Box::new(Assignable::Variable(stack(res), 0));
                    let from = Box::new(Expr::BinaryOp(
                        convert_bin_op(op),
                        Box::new(mk_variable(stack(v))),
                        Box::new(mk_variable(stack(w))),
                    ));
                    vec![stmt_expr(Expr::Assign {
                        to: to,
                        op: None,
                        from: from,
                    })]
                }
                Arithm::IncreaseLocal {
                    local_index,
                    increase,
                } => {
                    let to = Box::new(Assignable::Variable(local(local_index as usize), 0));
                    let increase = Box::new(Expr::Literal(Literal::Integer(increase as i32)));
                    let from = Box::new(Expr::BinaryOp(
                        BinOp::Add,
                        Box::new(Expr::Assignable(to.clone())),
                        increase,
                    ));
                    vec![stmt_expr(Expr::Assign {
                        to: to,
                        op: None,
                        from: from,
                    })]
                }
            },
            Instruction::TypeConv(_) => unimplemented!(),
            Instruction::ObjManip(_) => unimplemented!(),
            Instruction::StackManage(_) => unimplemented!(),
            Instruction::Jump(_) => unreachable!(),
            Instruction::Invoke(Invoke { method_index, kind }) => {
                let method_ref = &metadata.method_refs[&method_index];
                let class_ref = &metadata.class_refs[&method_ref.class_ref];
                let args_count = method_ref.signature.parameters.len() as isize;
                let args_range = self.0 - args_count..self.0;
                self.0 -= args_count;
                let this_object = match kind {
                    InvokeKind::Special | InvokeKind::Virtual => {
                        let top = self.pop();
                        Some(Box::new(mk_variable(stack(top))))
                    }
                    _ => None,
                };
                let method_call = Expr::Invoke(
                    this_object,
                    method_ref.clone(),
                    class_ref.clone(),
                    args_range
                        .into_iter()
                        .map(|i| mk_variable(stack(i)))
                        .collect::<Vec<_>>(),
                );
                if method_ref.signature.return_type == Type::Void {
                    vec![stmt_expr(method_call)]
                } else {
                    let result = self.push();
                    vec![stmt_expr(Expr::Assign {
                        from: Box::new(method_call),
                        op: None,
                        to: Box::new(Assignable::Variable(stack(result), 0)),
                    })]
                }
            }
            Instruction::Throw => unimplemented!(),
            Instruction::Return(value) => {
                let value = value.map(|_| {
                    let top = self.pop();
                    mk_variable(stack(top))
                });
                vec![Statement::Return(value)]
            }
            Instruction::Synchronized(_) => unimplemented!(),
        }
    }

    fn make_stack_vars_rvalue(&mut self, expr: &RValue, metadata: &Metadata) -> Expr {
        match *expr {
            RValue::Constant(ref literal) => Expr::Literal(literal.clone()),
            RValue::ConstantRef { const_ref } => {
                Expr::Literal(metadata.literals[&const_ref].clone())
            }
            RValue::LValue(ref lvalue) => {
                Expr::Assignable(Box::new(self.make_stack_vars_lvalue(lvalue, metadata)))
            }
        }
    }

    fn make_stack_vars_lvalue(&mut self, expr: &LValue, metadata: &Metadata) -> Assignable {
        let mut remove = 0;
        let result = match *expr {
            LValue::Local(index) => Assignable::Variable(local(index), 0),
            LValue::Stack(index) => {
                let real_index = self.get(index);
                remove += 1;
                Assignable::Variable(stack(real_index), 0)
            }
            LValue::StaticField { field_ref } => {
                let field = &metadata.field_refs[&field_ref];
                let class = &metadata.class_refs[&field.class_ref];
                Assignable::Field {
                    this: None,
                    class: class.clone(),
                    field: field.clone(),
                }
            }
            LValue::InstanceField {
                object_stack_index,
                field_ref,
            } => {
                let field = &metadata.field_refs[&field_ref];
                let class = &metadata.class_refs[&field.class_ref];
                let index = self.get(object_stack_index);
                remove += 1;
                Assignable::Field {
                    this: Some(Box::new(mk_variable(stack(index)))),
                    class: class.clone(),
                    field: field.clone(),
                }
            }
        };
        self.0 -= remove;
        result
    }

    pub fn cond_to_expr(&mut self, cond: &JumpCondition) -> Expr {
        match *cond {
            JumpCondition::CmpZero(ord) => {
                let v = self.pop();
                Expr::BinaryOp(
                    BinOp::Cmp(ord),
                    Box::new(mk_variable(stack(v))),
                    Box::new(Expr::Literal(Literal::Integer(0))),
                )
            }
            JumpCondition::Cmp(ord) | JumpCondition::CmpRef(ord) => {
                let w = self.pop();
                let v = self.pop();
                Expr::BinaryOp(
                    BinOp::Cmp(ord),
                    Box::new(mk_variable(stack(v))),
                    Box::new(mk_variable(stack(w))),
                )
            }
        }
    }
}

fn stack(i: isize) -> String {
    format!("stack_{}", i)
}

fn local(i: usize) -> String {
    format!("local_{}", i)
}

pub fn stack_to_vars(
    unit: CompilationUnit<Cfg<Instruction, JumpCondition>>,
) -> CompilationUnit<Cfg<Statement, Expr>> {
    let mut unit = unit.map(transform);
    for declaration in &mut unit.declarations {
        match *declaration {
            Declaration::Method(ref mut method) => {
                handle_parameters(method);
            }
            Declaration::Constructor(..) => unreachable!("no constructors at this point"),
            Declaration::Field(..) => {}
        }
    }
    unit
}

fn transform(
    mut cfg: Cfg<Instruction, JumpCondition>,
    metadata: &Metadata,
) -> Cfg<Statement, Expr> {
    use petgraph::visit::Dfs;
    let mut stack_at_bb = vec![None; cfg.graph.node_count()];
    stack_at_bb[0] = Some(StackLayout::new());
    let mut new_bbs = vec![BasicBlock::default(); cfg.graph.node_count()];
    let mut dfs = Dfs::new(&cfg.graph, NodeIndex::new(0));
    while let Some(v) = dfs.next(&cfg.graph) {
        let index = v.index();
        let mut stack = stack_at_bb[index].unwrap();
        new_bbs[index] = {
            let bb = &mut cfg.graph[v];
            let mut new_bb = BasicBlock::default();
            for inst in &mut bb.stmts {
                new_bb.stmts.append(&mut stack.execute(inst, metadata));
            }
            new_bb.terminator = bb.terminator.map(|t| stack.cond_to_expr(&t));
            new_bb
        };
        for w in cfg.graph.neighbors_directed(v, Direction::Outgoing) {
            let stack_at_w = &mut stack_at_bb[w.index()];
            if let Some(stack_at_w) = *stack_at_w {
                // Assert that all paths to w result in the same stack size:
                assert_eq!(
                    stack,
                    stack_at_w,
                    "expected stack {:?} at beginning of node #{} but found {:?}",
                    stack,
                    w.index(),
                    stack_at_w
                );
            } else {
                *stack_at_w = Some(stack);
            }
        }
    }
    use std::mem;
    Cfg {
        graph: cfg.graph.map(
            |nx, _| mem::replace(&mut new_bbs[nx.index()], BasicBlock::default()),
            |_, e| *e,
        ),
        entry_point: cfg.entry_point,
        exit_point: cfg.exit_point,
    }
}

fn handle_parameters(method: &mut Method<Cfg<Statement, Expr>>) {
    let mut local_index = 0;
    let mut assignments = vec![];
    if !method.modifiers.contains(&Modifier::Static) {
        let to = Box::new(Assignable::Variable(local(local_index), 0));
        let from = Box::new(Expr::This);
        assignments.push(Statement::Expr(Expr::Assign {
            from: from,
            op: None,
            to: to,
        }));
        local_index += 1;
    }
    for parameter in method.signature.parameters.iter_mut() {
        parameter.0 = local(local_index);
        local_index += 1;
    }
    if let Some(ref mut cfg) = method.code {
        let entry_block = &mut cfg.graph.node_weight_mut(cfg.entry_point).unwrap();
        entry_block.stmts.append(&mut assignments);
    }
}
