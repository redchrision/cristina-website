/* ============================================================
   CONTENT — typed view over the shared JSON in /content.
   The same files are embedded in the Rust server and served by
   the public /v1 API, so the site and the API never drift apart.
   ============================================================ */

import projectsJson from "../../content/projects.json";
import talksJson from "../../content/talks.json";
import speakerTopicsJson from "../../content/speaker_topics.json";
import blogJson from "../../content/blog.json";

export type Lang = "en" | "fr";

/** A value that is either language-neutral or has one string per language. */
export type Bi = string | Record<Lang, string>;

export interface ProjectLink {
  label: Bi;
  href: string;
}

export interface Project {
  mystery?: boolean;
  name: Bi;
  role: Bi;
  blurb: Bi;
  tags: Bi[];
  links: ProjectLink[];
}

export interface Talk {
  year: string;
  lang: string;
  event: string;
  title: Bi;
  theme: Bi;
  description: Bi;
  link?: string;
  abstract?: Bi;
}

export interface BlogBlock {
  t: "p" | "h2";
  x: string;
}

export interface BlogPost {
  slug: string;
  date: Bi;
  title: Bi;
  tags: string[];
  excerpt: Bi;
  body: Record<Lang, BlogBlock[]>;
}

/* ---------- Projects ----------
   Order matters: the first VISIBLE_PROJECTS show by default,
   the rest sit behind "See more". */
export const PROJECTS = projectsJson as Project[];

/* ---------- Talks ----------
   Newest first. The first 4 show by default; the rest sit behind
   "Show earlier talks". See content/README.md for items to verify. */
export const TALKS = talksJson as Talk[];

export const SPEAKER_TOPICS = speakerTopicsJson as Record<Lang, string[]>;

/* ---------- Blog ---------- */
export const BLOG_TAGS: string[] = blogJson.tags;
export const TAG_LABELS = blogJson.tagLabels as { fr: Record<string, string> };
export const BLOG_POSTS = blogJson.posts as unknown as BlogPost[];

/** pick a bilingual value */
export const t = (v: Bi, lang: Lang): string => (typeof v === "string" ? v : v[lang]);

export const tagLabel = (tag: string, lang: Lang): string =>
  lang === "fr" ? TAG_LABELS.fr[tag] || tag : tag;
