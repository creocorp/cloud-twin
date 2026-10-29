//! Minimal XML DOM for parsing AWS request bodies.
//!
//! S3 configuration documents (notifications, CORS, tagging, multipart
//! completion, ...) arrive as small XML payloads. This parses them into a tiny
//! tree with namespace prefixes stripped so handlers can navigate by local name.

use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    /// First direct child with the given local name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// All direct children with the given local name.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Trimmed text of the first direct child with the given name.
    pub fn child_text(&self, name: &str) -> Option<String> {
        self.child(name).map(|c| c.text.trim().to_string())
    }

    /// Follow a path of child names, e.g. `["Filter", "S3Key"]`.
    pub fn path(&self, names: &[&str]) -> Option<&Element> {
        let mut cur = self;
        for n in names {
            cur = cur.child(n)?;
        }
        Some(cur)
    }
}

fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    match s.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => s.to_string(),
    }
}

/// Parse an XML document and return its root element.
pub fn parse(input: &[u8]) -> Result<Element, String> {
    let mut reader = Reader::from_reader(input);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let attrs = e
                    .attributes()
                    .flatten()
                    .map(|a| {
                        (
                            local_name(a.key.as_ref()),
                            a.unescape_value()
                                .map(|v| v.to_string())
                                .unwrap_or_default(),
                        )
                    })
                    .collect();
                stack.push(Element {
                    name: local_name(e.name().as_ref()),
                    attrs,
                    ..Default::default()
                });
            }
            Ok(Event::Empty(e)) => {
                let el = Element {
                    name: local_name(e.name().as_ref()),
                    ..Default::default()
                };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => root = Some(el),
                }
            }
            Ok(Event::End(_)) => {
                let el = stack.pop().ok_or("unbalanced XML")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => root = Some(el),
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(cur) = stack.last_mut() {
                    let text = t.unescape().map_err(|e| e.to_string())?;
                    cur.text.push_str(&text);
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(cur) = stack.last_mut() {
                    cur.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
        buf.clear();
    }
    root.ok_or_else(|| "empty XML document".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_documents_and_strips_namespaces() {
        let doc = br#"<?xml version="1.0"?>
<Delete xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Object><Key>a &amp; b</Key></Object>
  <Object><Key>c</Key><VersionId>v1</VersionId></Object>
  <Quiet>true</Quiet>
  <Empty/>
</Delete>"#;
        let root = parse(doc).unwrap();
        assert_eq!(root.name, "Delete");
        let keys: Vec<String> = root
            .children_named("Object")
            .filter_map(|o| o.child_text("Key"))
            .collect();
        assert_eq!(keys, vec!["a & b", "c"]);
        assert_eq!(root.child_text("Quiet").as_deref(), Some("true"));
        assert!(root.child("Empty").is_some());
    }
}
