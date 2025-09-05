use schemars::Schema;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Debug)]
pub enum Expr {
    Literal(String),
    Ref(String),
    Seq(Vec<Expr>),
    Choice(Vec<Expr>),
    Repeat(Box<Expr>),
    Optional(Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub name: String,
    pub expr: Expr,
}

#[derive(Clone, Debug, Default)]
pub struct Grammar {
    pub rules: Vec<Rule>,
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Literal(s) => {
                f.write_char('"')?;
                for ch in s.chars() {
                    if ch == '"' || ch == '\\' {
                        f.write_char('\\')?;
                    }
                    f.write_char(ch)?;
                }
                f.write_char('"')
            }
            Expr::Ref(s) => f.write_str(s),
            Expr::Seq(v) => {
                for (i, e) in v.iter().enumerate() {
                    if i > 0 {
                        f.write_char(' ')?;
                    }
                    write!(f, "{}", e)?;
                }
                Ok(())
            }
            Expr::Choice(v) => {
                for (i, e) in v.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    write!(f, "{}", e)?;
                }
                Ok(())
            }
            Expr::Repeat(e) => write!(f, "({})*", e),
            Expr::Optional(e) => write!(f, "({})?", e),
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ::= {}", self.name, self.expr)
    }
}

impl fmt::Display for Grammar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, rule) in self.rules.iter().enumerate() {
            if i > 0 {
                f.write_char('\n')?;
            }
            write!(f, "{}", rule)?;
        }
        Ok(())
    }
}

pub struct Generator {
    counter: AtomicUsize,
}

impl Generator {
    pub fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }

    fn unique(&self) -> String {
        format!("r{}", self.counter.fetch_add(1, Ordering::Relaxed))
    }

    fn sanitize(&self, name: &str) -> String {
        let mut out = String::new();
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                out.push(ch);
            } else {
                out.push('-');
            }
        }
        if out.is_empty() || !out.chars().next().unwrap().is_ascii_alphabetic() {
            out.insert(0, 'r');
        }
        out
    }

    pub fn generate(&self, name: &str, schema: &Schema) -> Grammar {
        let mut grammar = Grammar::default();
        let defs: BTreeMap<String, Schema> = schema
            .as_object()
            .and_then(|o| o.get("$defs").or_else(|| o.get("definitions")))
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.clone().try_into().ok().map(|s| (k.clone(), s)))
                    .collect()
            })
            .unwrap_or_default();
        let mut cache = HashMap::new();
        let rule_name = format!("{}-{}", self.sanitize(name), self.unique());
        let expr = self.expr_from_schema(schema, &defs, &mut cache, &mut grammar);
        grammar.rules.insert(
            0,
            Rule {
                name: rule_name,
                expr,
            },
        );
        grammar
    }

    fn maybe_rule(&self, expr: Expr, grammar: &mut Grammar) -> Expr {
        match expr {
            Expr::Ref(_) | Expr::Literal(_) => expr,
            other => {
                let name = self.unique();
                grammar.rules.push(Rule {
                    name: name.clone(),
                    expr: other,
                });
                Expr::Ref(name)
            }
        }
    }

    fn expr_from_schema(
        &self,
        schema: &Schema,
        defs: &BTreeMap<String, Schema>,
        cache: &mut HashMap<String, String>,
        grammar: &mut Grammar,
    ) -> Expr {
        if let Some(obj) = schema.as_object() {
            if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
                if let Some(name) = reference
                    .strip_prefix("#/$defs/")
                    .or_else(|| reference.strip_prefix("#/definitions/"))
                {
                    if let Some(existing) = cache.get(name) {
                        return Expr::Ref(existing.clone());
                    }
                    if let Some(resolved) = defs.get(name) {
                        let expr = self.expr_from_schema(resolved, defs, cache, grammar);
                        let expr = self.maybe_rule(expr, grammar);
                        if let Expr::Ref(rule) = &expr {
                            cache.insert(name.to_string(), rule.clone());
                        }
                        return expr;
                    }
                }
            }
        }

        let ty = schema
            .as_object()
            .and_then(|o| o.get("type"))
            .and_then(|t| match t {
                Value::String(s) => Some(s.clone()),
                Value::Array(arr) => arr
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|s| *s != "null")
                    .map(|s| s.to_string()),
                _ => None,
            });

        match ty.as_deref() {
            Some("string") => Expr::Ref("string".into()),
            Some("number") | Some("integer") => Expr::Ref("number".into()),
            Some("boolean") => Expr::Choice(vec![
                Expr::Literal("true".into()),
                Expr::Literal("false".into()),
            ]),
            Some("object") => self.object_expr(schema, defs, cache, grammar),
            Some("array") => self.array_expr(schema, defs, cache, grammar),
            _ => Expr::Ref("json".into()),
        }
    }

    fn object_expr(
        &self,
        schema: &Schema,
        defs: &BTreeMap<String, Schema>,
        cache: &mut HashMap<String, String>,
        grammar: &mut Grammar,
    ) -> Expr {
        let obj = match schema.as_object() {
            Some(o) => o,
            None => return Expr::Ref("json".into()),
        };
        let props = obj.get("properties").and_then(Value::as_object);
        let required: Vec<String> = obj
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut seq = Vec::new();
        seq.push(Expr::Literal("{".into()));
        seq.push(Expr::Ref("ws".into()));
        let mut first = true;
        if let Some(map) = props {
            let mut required_props: Vec<_> = map
                .iter()
                .filter(|(name, _)| required.contains(*name))
                .collect();
            required_props.sort_by(|a, b| a.0.cmp(b.0));
            for (name, subschema) in required_props {
                if !first {
                    seq.push(Expr::Literal(",".into()));
                    seq.push(Expr::Ref("ws".into()));
                }
                first = false;
                seq.push(Expr::Literal(format!("\"{}\"", name)));
                seq.push(Expr::Ref("ws".into()));
                seq.push(Expr::Literal(":".into()));
                seq.push(Expr::Ref("ws".into()));
                let subschema: Schema = subschema.clone().try_into().unwrap_or_default();
                let expr = self.expr_from_schema(&subschema, defs, cache, grammar);
                seq.push(self.maybe_rule(expr, grammar));
            }

            let mut optional_props: Vec<_> = map
                .iter()
                .filter(|(name, _)| !required.contains(*name))
                .collect();
            optional_props.sort_by(|a, b| a.0.cmp(b.0));

            if !optional_props.is_empty() {
                let mut choices = Vec::new();
                for (name, subschema) in optional_props {
                    let subschema: Schema = subschema.clone().try_into().unwrap_or_default();
                    let expr = self.expr_from_schema(&subschema, defs, cache, grammar);
                    let expr = self.maybe_rule(expr, grammar);
                    choices.push(Expr::Seq(vec![
                        Expr::Literal(format!("\"{}\"", name)),
                        Expr::Ref("ws".into()),
                        Expr::Literal(":".into()),
                        Expr::Ref("ws".into()),
                        expr,
                    ]));
                }

                if first {
                    seq.push(Expr::Optional(Box::new(Expr::Seq(vec![
                        Expr::Choice(choices.clone()),
                        Expr::Repeat(Box::new(Expr::Seq(vec![
                            Expr::Literal(",".into()),
                            Expr::Ref("ws".into()),
                            Expr::Choice(choices),
                        ]))),
                    ]))));
                } else {
                    seq.push(Expr::Repeat(Box::new(Expr::Seq(vec![
                        Expr::Literal(",".into()),
                        Expr::Ref("ws".into()),
                        Expr::Choice(choices),
                    ]))));
                }
            }
        }
        seq.push(Expr::Ref("ws".into()));
        seq.push(Expr::Literal("}".into()));
        Expr::Seq(seq)
    }

    fn array_expr(
        &self,
        schema: &Schema,
        defs: &BTreeMap<String, Schema>,
        cache: &mut HashMap<String, String>,
        grammar: &mut Grammar,
    ) -> Expr {
        let items_schema = schema.as_object().and_then(|o| o.get("items")).cloned();
        let items_schema: Schema = match items_schema {
            Some(s) => s.try_into().unwrap_or_default(),
            None => return Expr::Ref("json".into()),
        };
        let item_expr = self.expr_from_schema(&items_schema, defs, cache, grammar);
        let item_expr = self.maybe_rule(item_expr, grammar);
        Expr::Seq(vec![
            Expr::Literal("[".into()),
            Expr::Ref("ws".into()),
            Expr::Optional(Box::new(Expr::Seq(vec![
                item_expr.clone(),
                Expr::Repeat(Box::new(Expr::Seq(vec![
                    Expr::Literal(",".into()),
                    Expr::Ref("ws".into()),
                    item_expr,
                ]))),
            ]))),
            Expr::Ref("ws".into()),
            Expr::Literal("]".into()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;

    #[derive(JsonSchema)]
    struct ReplaceParams {
        file_path: String,
        new_string: String,
        old_string: String,
        expected_replacements: Option<usize>,
    }

    #[test]
    fn replace_params_grammar() {
        let schema = schemars::schema_for!(ReplaceParams);
        let generator = Generator::new();
        let grammar = generator.generate("params", &schema);
        insta::assert_snapshot!(grammar.to_string(), @r###"
params-r0 ::= "{" ws "\"file_path\"" ws ":" ws string "," ws "\"new_string\"" ws ":" ws string "," ws "\"old_string\"" ws ":" ws string ("," ws "\"expected_replacements\"" ws ":" ws number)* ws "}"
"###);
    }

    #[derive(JsonSchema)]
    struct ListDirectoryParams {
        path: String,
        ignore: Option<Vec<String>>,
        include: Option<Vec<String>>,
        include_hidden: Option<bool>,
    }

    #[test]
    fn list_directory_params_grammar() {
        let schema = schemars::schema_for!(ListDirectoryParams);
        let generator = Generator::new();
        let grammar = generator.generate("params", &schema);
        insta::assert_snapshot!(grammar.to_string(), @r###"
params-r0 ::= "{" ws "\"path\"" ws ":" ws string ("," ws "\"ignore\"" ws ":" ws r1 | "\"include\"" ws ":" ws r2 | "\"include_hidden\"" ws ":" ws r3)* ws "}"
r1 ::= "[" ws (string ("," ws string)*)? ws "]"
r2 ::= "[" ws (string ("," ws string)*)? ws "]"
r3 ::= "true" | "false"
"###);
    }

    #[derive(JsonSchema)]
    struct Item {
        value: String,
    }

    #[derive(JsonSchema)]
    struct Container {
        item: Item,
        items: Vec<Item>,
    }

    #[test]
    fn nested_structs_grammar() {
        let schema = schemars::schema_for!(Container);
        let generator = Generator::new();
        let grammar = generator.generate("params", &schema);
        insta::assert_snapshot!(grammar.to_string(), @r###"
params-r0 ::= "{" ws "\"item\"" ws ":" ws r1 "," ws "\"items\"" ws ":" ws r2 ws "}"
r1 ::= "{" ws "\"value\"" ws ":" ws string ws "}"
r2 ::= "[" ws (r1 ("," ws r1)*)? ws "]"
"###);
    }
}
