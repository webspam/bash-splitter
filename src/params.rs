use brush_parser::word;

/// The parameter an expansion acts on (`x` in `${x:-y}`), or `None` for the
/// prefix/key listings (`${!pre*}`, `${!arr[@]}`), which name no single parameter.
pub(crate) fn param_of(expr: &word::ParameterExpr) -> Option<&word::Parameter> {
    use word::ParameterExpr as P;
    match expr {
        P::Parameter { parameter, .. }
        | P::UseDefaultValues { parameter, .. }
        | P::AssignDefaultValues { parameter, .. }
        | P::IndicateErrorIfNullOrUnset { parameter, .. }
        | P::UseAlternativeValue { parameter, .. }
        | P::ParameterLength { parameter, .. }
        | P::RemoveSmallestSuffixPattern { parameter, .. }
        | P::RemoveLargestSuffixPattern { parameter, .. }
        | P::RemoveSmallestPrefixPattern { parameter, .. }
        | P::RemoveLargestPrefixPattern { parameter, .. }
        | P::Substring { parameter, .. }
        | P::Transform { parameter, .. }
        | P::UppercaseFirstChar { parameter, .. }
        | P::UppercasePattern { parameter, .. }
        | P::LowercaseFirstChar { parameter, .. }
        | P::LowercasePattern { parameter, .. }
        | P::ReplaceSubstring { parameter, .. } => Some(parameter),
        P::VariableNames { .. } | P::MemberKeys { .. } => None,
    }
}

/// The name of the variable a parameter references; positionals and specials render
/// as their token (`1`, `?`).
pub(crate) fn param_name(parameter: &word::Parameter) -> String {
    use word::Parameter as P;
    match parameter {
        P::Named(name)
        | P::NamedWithIndex { name, .. }
        | P::NamedWithAllIndices { name, .. } => name.clone(),
        P::Positional(n) => n.to_string(),
        P::Special(s) => s.to_string(),
    }
}

/// The subscript text of an array-indexed parameter (`arr[idx]`), if any. The
/// index is arithmetic-evaluated, so a substitution in it runs (`${arr[$(cmd)]}`).
pub(crate) fn param_subscript(expr: &word::ParameterExpr) -> Option<&str> {
    match param_of(expr)? {
        word::Parameter::NamedWithIndex { index, .. } => Some(index),
        _ => None,
    }
}

/// A parameter expansion's value and pattern words are themselves expanded, so a
/// command or variable in one is reachable (`${x:-$(cmd)}`, `${x/$y/z}`, `${x:$n}`).
/// `sink` receives each such word.
pub(crate) fn collect_param_expr_words(expr: &word::ParameterExpr, mut sink: impl FnMut(&str)) {
    use word::ParameterExpr as P;
    match expr {
        P::UseDefaultValues {
            default_value: Some(s),
            ..
        }
        | P::AssignDefaultValues {
            default_value: Some(s),
            ..
        }
        | P::UseAlternativeValue {
            alternative_value: Some(s),
            ..
        }
        | P::IndicateErrorIfNullOrUnset {
            error_message: Some(s),
            ..
        }
        | P::RemoveSmallestSuffixPattern {
            pattern: Some(s), ..
        }
        | P::RemoveLargestSuffixPattern {
            pattern: Some(s), ..
        }
        | P::RemoveSmallestPrefixPattern {
            pattern: Some(s), ..
        }
        | P::RemoveLargestPrefixPattern {
            pattern: Some(s), ..
        }
        | P::UppercaseFirstChar {
            pattern: Some(s), ..
        }
        | P::UppercasePattern {
            pattern: Some(s), ..
        }
        | P::LowercaseFirstChar {
            pattern: Some(s), ..
        }
        | P::LowercasePattern {
            pattern: Some(s), ..
        } => sink(s),
        P::ReplaceSubstring {
            pattern,
            replacement,
            ..
        } => {
            sink(pattern);
            if let Some(r) = replacement {
                sink(r);
            }
        }
        P::Substring { offset, length, .. } => {
            sink(&offset.value);
            if let Some(l) = length {
                sink(&l.value);
            }
        }
        _ => {}
    }
}
