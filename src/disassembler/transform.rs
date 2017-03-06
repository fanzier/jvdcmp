pub use super::super::classfile::parser::*;
pub use super::class::*;
pub use super::disassembler::*;
use std::collections::HashMap;

pub fn transform(class_file: ClassFile) -> CompilationUnit {
    let mut unit = CompilationUnit {
        typ: if class_file.access_flags.contains(ACC_INTERFACE) {
            UnitType::Interface
        } else if class_file.access_flags.contains(ACC_ENUM) {
            UnitType::Enum
        } else {
            UnitType::Class
        },
        modifiers: vec![],
        declarations: vec![],
        java_constants: HashMap::new(),
        string_constants: HashMap::new(),
        class_refs: HashMap::new(),
        field_refs: HashMap::new(),
        method_refs: HashMap::new(),
        name_refs: HashMap::new(),
    };
    unit.modifiers = class_flags_to_modifiers(&class_file.access_flags);
    process_constant_pool(&mut unit, class_file.constant_pool);
    process_methods(&mut unit, &class_file.methods);
    unit
}

fn class_flags_to_modifiers(flags: &AccessFlags) -> Vec<Modifier> {
    let mut modifiers = vec![];
    if flags.contains(ACC_PUBLIC) {
        modifiers.push(Modifier::Public);
    }
    if flags.contains(ACC_PROTECTED) {
        modifiers.push(Modifier::Protected);
    }
    if flags.contains(ACC_PRIVATE) {
        modifiers.push(Modifier::Private);
    }
    if flags.contains(ACC_STATIC) {
        modifiers.push(Modifier::Static);
    }
    if flags.contains(ACC_ABSTRACT) {
        modifiers.push(Modifier::Abstract);
    }
    if flags.contains(ACC_FINAL) {
        modifiers.push(Modifier::Final);
    }
    modifiers
}

fn process_constant_pool(unit: &mut CompilationUnit, constant_pool: ConstantPool) {
    for (index, constant) in constant_pool.constants.iter().enumerate() {
        let index = index as u16 + 1; // plus one because of weird indexing in the JVM spec
        match *constant {
            ConstantInfo::Utf8(ref str) => {
                unit.string_constants.insert(index, str.to_owned());
            }
            ConstantInfo::Integer(int) => {
                unit.java_constants.insert(index, JavaConstant::Integer(int));
            }
            ConstantInfo::Class { name_index } => {
                let name = constant_pool.lookup_string(name_index);
                unit.class_refs.insert(index, ClassRef(name.to_owned()));
            }
            ConstantInfo::String { string_index } => {
                let string = constant_pool.lookup_string(string_index);
                unit.java_constants.insert(index, JavaConstant::String(string.to_owned()));
            }
            ConstantInfo::FieldRef { class_index, name_index } => {
                let (name_index, descriptor_index) = match *constant_pool.lookup(name_index) {
                    ConstantInfo::NameAndType { name_index, descriptor_index } => {
                        (name_index, descriptor_index)
                    }
                    ref c => panic!("Index doesn't point to a NameAndType but to: {:#?}", c),
                };
                let name = constant_pool.lookup_string(name_index).to_owned();
                let descriptor = constant_pool.lookup_string(descriptor_index);
                let typ = descriptor_to_type(&mut descriptor.chars());
                unit.field_refs.insert(index,
                                       FieldRef {
                                           class_ref: class_index,
                                           name: name,
                                           typ: typ,
                                       });
            }
            ConstantInfo::MethodRef { class_index, name_index } => {
                let (name_index, descriptor_index) = match *constant_pool.lookup(name_index) {
                    ConstantInfo::NameAndType { name_index, descriptor_index } => {
                        (name_index, descriptor_index)
                    }
                    ref c => panic!("Index doesn't point to a NameAndType but to: {:#?}", c),
                };
                let name = constant_pool.lookup_string(name_index).to_owned();
                let descriptor = constant_pool.lookup_string(descriptor_index);
                let signature = descriptor_to_signature(descriptor);
                unit.method_refs.insert(index,
                                        MethodRef {
                                            class_ref: class_index,
                                            name: name,
                                            signature: signature,
                                        });
            }
            ConstantInfo::NameAndType { name_index, descriptor_index } => {
                let name = constant_pool.lookup_string(name_index).to_owned();
                let descriptor_string = constant_pool.lookup_string(descriptor_index);
                let descriptor = if descriptor_string.chars().next() == Some('(') {
                    Descriptor::Signature(descriptor_to_signature(descriptor_string))
                } else {
                    Descriptor::Type(descriptor_to_type(&mut descriptor_string.chars()))
                };
                unit.name_refs.insert(index,
                                      NameRef {
                                          name: name,
                                          typ: descriptor,
                                      });
            }
        }
    }
}

fn process_methods(unit: &mut CompilationUnit, methods: &[MethodInfo]) {
    for method in methods {
        let transformed = transform_method(&unit, &method);
        unit.declarations.push(transformed);
    }
}

fn transform_method(unit: &CompilationUnit, method: &MethodInfo) -> Declaration {
    let mut code = None;
    for attribute in &method.attributes {
        let name = unit.lookup_string(attribute.name_index);
        if name == "Code" {
            let code_attribute = parse_code_attribute(&attribute.info).unwrap();
            let disassembly = disassemble(unit, code_attribute);
            code = Some(disassembly);
            break;
        }
    }
    Declaration::Method {
        modifiers: method_flags_to_modifiers(&method.access_flags),
        name: unit.lookup_string(method.name_index).to_owned(),
        signature: descriptor_to_signature(unit.lookup_string(method.descriptor_index)),
        code: code,
    }
}

fn method_flags_to_modifiers(flags: &AccessFlags) -> Vec<Modifier> {
    let mut modifiers = vec![];
    if flags.contains(ACC_PUBLIC) {
        modifiers.push(Modifier::Public);
    }
    if flags.contains(ACC_PROTECTED) {
        modifiers.push(Modifier::Protected);
    }
    if flags.contains(ACC_PRIVATE) {
        modifiers.push(Modifier::Private);
    }
    if flags.contains(ACC_STATIC) {
        modifiers.push(Modifier::Static);
    }
    if flags.contains(ACC_ABSTRACT) {
        modifiers.push(Modifier::Abstract);
    }
    if flags.contains(ACC_FINAL) {
        modifiers.push(Modifier::Final);
    }
    // Method specific flags:
    if flags.contains(ACC_SYNCHRONIZED) {
        modifiers.push(Modifier::Synchronized);
    }
    if flags.contains(ACC_NATIVE) {
        modifiers.push(Modifier::Native);
    }
    if flags.contains(ACC_STRICT) {
        modifiers.push(Modifier::Strictfp);
    }
    modifiers
}

fn descriptor_to_signature(descriptor: &str) -> Signature {
    let mut chars = descriptor.chars().peekable();
    let mut params = vec![];
    let next = chars.next().unwrap();
    if next != '(' {
        panic!("Expected open paren at beginning of method descriptor: {:?}",
               descriptor);
    }
    loop {
        let lookahead = *chars.peek().unwrap();
        if lookahead != ')' {
            params.push(descriptor_to_type(&mut chars));
        }
        let next = chars.next().unwrap();
        match next {
            ')' => break,
            ',' => (),
            _ => panic!("Expected ')' or ',' in method descriptor: {:?}", descriptor),
        }
    }
    let return_type = descriptor_to_type(&mut chars);
    Signature {
        parameters: params,
        return_type: return_type,
    }
}

fn descriptor_to_type<I: Iterator<Item = char>>(chars: &mut I) -> Type {
    let next = chars.next().unwrap();
    match next {
        'B' => Type::Byte,
        'C' => Type::Char,
        'D' => Type::Double,
        'F' => Type::Float,
        'I' => Type::Int,
        'J' => Type::Long,
        'L' => {
            let mut class_name = String::new();
            for ch in chars {
                if ch == ';' {
                    break;
                }
                class_name.push(ch);
            }
            Type::Reference(class_name)
        }
        'S' => Type::Short,
        'V' => Type::Void,
        'Z' => Type::Boolean,
        '[' => Type::Array(Box::new(descriptor_to_type(chars))),
        _ => panic!("Invalid start of type descriptor: {:?}", next),
    }
}
