# Site content

Single source of truth for the projects, talks and blog posts. These JSON
files are imported by the React frontend (`web/src/content.ts`) **and**
embedded into the Rust server at compile time (`server/src/content.rs`), so the
website, the `/v1` API and the MCP tools always show the same thing.

Bilingual fields are either a plain string or `{ "en": "...", "fr": "..." }`.

| File | What |
|---|---|
| `projects.json` | Projects, most important first (the first 4 show by default) |
| `talks.json` | Talks, newest first (the first 4 show by default) |
| `speaker_topics.json` | "Topics I speak about" list |
| `blog.json` | Blog tags, French tag labels, and posts |

## To verify (notes carried over from the old source comments)

- `talks.json`, first talk (Women's Day 2026 panel): confirm the YouTube link
  is the panel recording.
- `talks.json`, second talk (Dark patterns): confirm the link is the
  dark-patterns recording.
- Event names in `[brackets]` are placeholders to fill in.
