//! Site content, embedded at compile time from `/content/*.json` — the very
//! same files the React frontend imports, so the API can never drift from
//! what the site shows.

use std::sync::LazyLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Language selector accepted by the content endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Fr,
}

/// Either one language-neutral string, or one string per language.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, JsonSchema)]
#[serde(untagged)]
pub enum Bi {
    Plain(String),
    Localized { en: String, fr: String },
}

impl Bi {
    pub fn get(&self, lang: Lang) -> &str {
        match (self, lang) {
            (Self::Plain(s), _) => s,
            (Self::Localized { en, .. }, Lang::En) => en,
            (Self::Localized { fr, .. }, Lang::Fr) => fr,
        }
    }

    /// Collapse to a plain string when a language was requested.
    fn localize(&mut self, lang: Option<Lang>) {
        if let Some(lang) = lang {
            *self = Self::Plain(self.get(lang).to_owned());
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, JsonSchema)]
pub struct ProjectLink {
    pub label: Bi,
    pub href: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, JsonSchema)]
pub struct Project {
    /// Highlighted on the site: the current main project.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub featured: bool,
    pub name: Bi,
    pub role: Bi,
    pub blurb: Bi,
    pub tags: Vec<Bi>,
    pub links: Vec<ProjectLink>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, JsonSchema)]
pub struct Talk {
    pub year: String,
    /// Language the talk was given in (EN / FR).
    pub lang: String,
    pub event: String,
    pub title: Bi,
    pub theme: Bi,
    pub description: Bi,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#abstract: Option<Bi>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, JsonSchema)]
pub struct BlogBlock {
    /// `p` (paragraph) or `h2` (heading).
    pub t: String,
    pub x: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, JsonSchema)]
pub struct BlogBody {
    pub en: Vec<BlogBlock>,
    pub fr: Vec<BlogBlock>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, JsonSchema)]
pub struct BlogPost {
    pub slug: String,
    pub date: Bi,
    pub title: Bi,
    pub tags: Vec<String>,
    pub excerpt: Bi,
    pub body: BlogBody,
}

/// A post without its body, for listings.
#[derive(Debug, Clone, Serialize, ToSchema, JsonSchema)]
pub struct BlogPostSummary {
    pub slug: String,
    pub date: Bi,
    pub title: Bi,
    pub tags: Vec<String>,
    pub excerpt: Bi,
}

#[derive(Debug, Deserialize)]
struct BlogFile {
    posts: Vec<BlogPost>,
}

pub struct Content {
    pub projects: Vec<Project>,
    pub talks: Vec<Talk>,
    pub posts: Vec<BlogPost>,
}

pub static CONTENT: LazyLock<Content> = LazyLock::new(|| Content {
    projects: serde_json::from_str(include_str!("../../content/projects.json"))
        .expect("content/projects.json is valid"),
    talks: serde_json::from_str(include_str!("../../content/talks.json"))
        .expect("content/talks.json is valid"),
    posts: serde_json::from_str::<BlogFile>(include_str!("../../content/blog.json"))
        .expect("content/blog.json is valid")
        .posts,
});

pub fn projects(lang: Option<Lang>) -> Vec<Project> {
    CONTENT
        .projects
        .iter()
        .cloned()
        .map(|mut p| {
            p.name.localize(lang);
            p.role.localize(lang);
            p.blurb.localize(lang);
            p.tags.iter_mut().for_each(|t| t.localize(lang));
            p.links.iter_mut().for_each(|l| l.label.localize(lang));
            p
        })
        .collect()
}

pub fn talks(lang: Option<Lang>) -> Vec<Talk> {
    CONTENT
        .talks
        .iter()
        .cloned()
        .map(|mut t| {
            t.title.localize(lang);
            t.theme.localize(lang);
            t.description.localize(lang);
            if let Some(a) = t.r#abstract.as_mut() {
                a.localize(lang);
            }
            t
        })
        .collect()
}

pub fn posts(lang: Option<Lang>, tag: Option<&str>) -> Vec<BlogPostSummary> {
    CONTENT
        .posts
        .iter()
        .filter(|p| tag.is_none_or(|tag| p.tags.iter().any(|t| t == tag)))
        .map(|p| {
            let mut s = BlogPostSummary {
                slug: p.slug.clone(),
                date: p.date.clone(),
                title: p.title.clone(),
                tags: p.tags.clone(),
                excerpt: p.excerpt.clone(),
            };
            s.date.localize(lang);
            s.title.localize(lang);
            s.excerpt.localize(lang);
            s
        })
        .collect()
}

pub fn post(slug: &str, lang: Option<Lang>) -> Option<BlogPost> {
    let mut p = CONTENT.posts.iter().find(|p| p.slug == slug)?.clone();
    p.date.localize(lang);
    p.title.localize(lang);
    p.excerpt.localize(lang);
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_content_parses() {
        assert!(!CONTENT.projects.is_empty());
        assert!(!CONTENT.talks.is_empty());
        assert!(!CONTENT.posts.is_empty());
    }

    #[test]
    fn localization_flattens_bilingual_fields() {
        let fr = projects(Some(Lang::Fr));
        assert!(matches!(fr[0].role, Bi::Plain(_)));
        let raw = projects(None);
        assert!(matches!(raw[0].role, Bi::Localized { .. }));
    }
}
