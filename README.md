# Cristina DeLisle — Portfolio

A single-page React portfolio built with Vite. Dark aubergine theme with a
generative "silk flow field" background, animated portrait ring, scrollspy
sidebar navigation, projects, talks, writing, booking and an accessible
contact form.

## Run it locally

Requires Node.js 18+.

```bash
npm install
npm run dev
```

Then open the URL Vite prints (usually http://localhost:5173).

## Build for production

```bash
npm run build     # outputs static files to dist/
npm run preview   # serve the production build locally
```

Deploy the `dist/` folder to any static host (Netlify, Cloudflare Pages,
OVH, etc.).

## Where to edit content

Everything lives at the top of `src/App.jsx`:

- `PHOTO_URL` — your portrait (the optimized crop is in `public/cristina.jpg`)
- `LINKS` — GitHub, LinkedIn, Mastodon, CV, **booking link** (create a
  Cal.com event and paste its URL), and `contactEndpoint` for the form
- `PROJECTS`, `TALKS`, `POSTS` — plain arrays; edit freely
- Talk titles are placeholders — replace them with your real titles

## Contact form

The form validates client-side and includes a honeypot anti-spam field,
but it needs a backend to actually deliver messages. Point
`LINKS.contactEndpoint` at a Formspree endpoint (or your own API) and it
will POST `{ name, email, message }` as JSON. Add server-side validation
and rate limiting there.

## Security headers (recommended at deploy time)

Serve over HTTPS and add: `Strict-Transport-Security`,
`X-Content-Type-Options: nosniff`, `Referrer-Policy:
strict-origin-when-cross-origin`, and a `Content-Security-Policy`
(the only external origin used is Google Fonts; self-host the fonts to
remove even that).
