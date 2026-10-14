use crate::Doc;

#[derive(Debug, PartialEq, Eq)]
pub struct Query {
    pub must: Vec<String>,
    pub not: Vec<String>,
}

pub fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

pub fn doc_terms(d: &Doc) -> Vec<String> {
    let mut t = tokenize(&d.message);
    t.push(format!("level:{}", d.level.to_lowercase()));
    t.push(format!("service:{}", d.service.to_lowercase()));
    t.sort();
    t.dedup();
    t
}

fn word_terms(w: &str) -> Vec<String> {
    if let Some((f, v)) = w.split_once(':') {
        let f = f.to_lowercase();
        if f == "level" || f == "service" {
            let v = v.to_lowercase();
            return if v.is_empty() {
                Vec::new()
            } else {
                vec![format!("{f}:{v}")]
            };
        }
    }
    tokenize(w)
}

pub fn parse(q: &str) -> Query {
    let mut must = Vec::new();
    let mut not = Vec::new();
    for word in q.split_whitespace() {
        let (neg, w) = match word.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, word),
        };
        let dst = if neg { &mut not } else { &mut must };
        dst.extend(word_terms(w));
    }
    must.sort();
    must.dedup();
    not.sort();
    not.dedup();
    Query { must, not }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_terms_exclusions_and_fields() {
        let q = parse("Timeout -retry level:ERROR service:Api");
        assert_eq!(q.must, vec!["level:error", "service:api", "timeout"]);
        assert_eq!(q.not, vec!["retry"]);
    }

    #[test]
    fn empty_and_punctuation_queries() {
        assert_eq!(parse(""), Query { must: vec![], not: vec![] });
        assert_eq!(parse("-"), Query { must: vec![], not: vec![] });
        assert_eq!(parse("db-error").must, vec!["db", "error"]);
    }
}