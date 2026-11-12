import { useEffect, useRef, useState } from "react";
import type { ChangeEvent, RefObject } from "react";
import { CONTACT_ENDPOINT, CONTACT_FORM_ON, trackPageview } from "./api";
import {
  BLOG_POSTS, BLOG_TAGS, PROJECTS, SPEAKER_TOPICS, TALKS, t, tagLabel,
} from "./content";
import type { Lang } from "./content";

type Theme = "light" | "dark";

/* ============================================================
   CRISTINA DELISLE — PORTFOLIO v7 · bilingual EN/FR
   ------------------------------------------------------------
   - Language toggle (top right), default follows the browser.
   - The featured project (current work) gets a highlighted card.
   - Projects, talks and blog posts live in /content/*.json (shared
     with the Rust API). UI strings, About and Legal live below.
     Bilingual fields are objects { en: "...", fr: "..." }.
   Routes: #/ home · #/blog · #/blog/:slug · #/legal
   ============================================================ */

const PHOTO_URL = "/cristina.jpg"; // optimized crop in /public

const LINKS = {
  github: "https://github.com/redchrision",
  linkedin: "https://www.linkedin.com/in/cristina-delisle-10848029/",
  mastodon: "https://mastodon.social/@redchrision",
  ellestime: "https://www.ellestime.fr/",
  email: "hello@cristinadelisle.xyz",
};

/* ---------- UI strings ---------- */
const UI = {
  en: {
    role: "R&D Software Engineer",
    tagline: ["I build open-source AI tools that ", "show their sources", " and run on your own hardware."],
    nav: { about: "About", projects: "Projects", talks: "Talks", mentorship: "Mentorship", contact: "Contact", blog: "Blog" },
    avail: "Open to consulting, speaking & mentorship",
    cvLabel: "Request my CV", cvSubject: "CV request",
    secAbout: "About", secProjects: "Projects", secTalks: "Talks", secMentorship: "Mentorship", secContact: "Get in touch",
    demo: "Demo ↗", repo: "Repository ↗", watch: "View the talk ↗",
    showEarlier: (n: number) => `Show ${n} earlier talk${n > 1 ? "s" : ""}`,
    hideEarlier: "Show recent talks only",
    seeMoreProjects: (n: number) => `See ${n} more project${n > 1 ? "s" : ""}`,
    seeFewerProjects: "See fewer projects",
    inviteTitle: "Invite me to speak", inviteTopics: "Topics I speak about:", inviteBtn: "Invite me ↗", inviteSubject: "Speaking invitation",
    mentorTitle: "One hour, every three months: for women and underrepresented people in tech",
    mentorIntro: ["As a Women Techmakers Ambassador and a mentor with ", "ElleStime", ", I keep a mentorship hour: one person, once per quarter, free. It is reserved for women and people from underrepresented groups in tech. Wherever you are on the road, it works best with something concrete on the table:"],
    mentorItems: [
      "New to tech or in a career transition: we review your CV, portfolio, or learning plan",
      "Building something: I review your project, code, or open-source repo",
      "Senior and wanting a second opinion: architecture, positioning, speaking, the next move",
    ],
    mentorApply: ["To apply, email me with ", "something to review", " (a repo link, a CV, a deck), plus a couple of lines about what you'd like to get out of the hour."],
    mentorNote: "If the quarter's slot is taken, I'll tell you when the next one opens. Not in one of these groups? My inbox is still open; this hour is just reserved.",
    mentorSubject: "Mentorship request",
    contactIntro: ["For opportunities, speaking invitations, project collaborations, or anything open source, write to me below, or directly at "],
    contactIntroNoForm: "For opportunities, speaking invitations, project collaborations, or anything open source, write to me at ",
    fName: "Name", fEmail: "Email", fMsg: "Message",
    eName: "Please enter your name.", eEmail: "Please enter a valid email address.", eMsg: "Please write a few words about your request.",
    sent: "Thank you, your message has been sent. I usually reply within two working days.",
    send: "Send message", sending: "Sending…",
    sendFail: "Sorry, the message could not be sent. Please try again, or write to me directly at",
    formNote: ["Sent directly to me, never shared. See the ", "privacy notice", "."],
    colophon: "Designed and built by Cristina DeLisle. Made with React. No cookies, no trackers.",
    legalLink: "Mentions légales & privacy",
    blogTitle: "Pieces of thought",
    blogSub: "Notes on open source, learning, communities, and the occasional demo or video, written down so it lasts longer than a feed.",
    filterPh: "Filter posts…", all: "all",
    empty: "Nothing matches. Try clearing the filter or the tag.",
    backHome: "← Cristina DeLisle", backPosts: "← All posts", notFound: "Post not found.",
    toTop: "Back to top", skip: "Skip to content",
    themeToLight: "Switch to light mode", themeToDark: "Switch to dark mode",
  },
  fr: {
    role: "Ingénieure logiciel R&D",
    tagline: ["Je construis des outils d'IA open source qui ", "montrent leurs sources", " et tournent sur votre propre matériel."],
    nav: { about: "À propos", projects: "Projets", talks: "Conférences", mentorship: "Mentorat", contact: "Contact", blog: "Blog" },
    avail: "Disponible pour du conseil, des conférences et du mentorat",
    cvLabel: "Demander mon CV", cvSubject: "Demande de CV",
    secAbout: "À propos", secProjects: "Projets", secTalks: "Conférences", secMentorship: "Mentorat", secContact: "Me contacter",
    demo: "Démo ↗", repo: "Dépôt ↗", watch: "Voir la conférence ↗",
    showEarlier: (n: number) => `Voir ${n} conférence${n > 1 ? "s" : ""} plus ancienne${n > 1 ? "s" : ""}`,
    hideEarlier: "N'afficher que les récentes",
    seeMoreProjects: (n: number) => `Voir ${n} projet${n > 1 ? "s" : ""} de plus`,
    seeFewerProjects: "Voir moins de projets",
    inviteTitle: "Invitez-moi à intervenir", inviteTopics: "Sujets sur lesquels j'interviens :", inviteBtn: "M'inviter ↗", inviteSubject: "Invitation à une conférence",
    mentorTitle: "Une heure par trimestre : pour les femmes et les personnes sous-représentées dans la tech",
    mentorIntro: ["Ambassadrice Women Techmakers et mentore chez ", "ElleStime", ", je réserve une heure de mentorat par trimestre : une personne à la fois, gratuitement. Elle est destinée aux femmes et aux personnes issues de groupes sous-représentés dans la tech. Où que vous en soyez, cela fonctionne mieux avec quelque chose de concret sur la table :"],
    mentorItems: [
      "Nouvelle dans la tech ou en reconversion : on relit votre CV, votre portfolio ou votre plan d'apprentissage",
      "En train de construire : je relis votre projet, votre code ou votre dépôt open source",
      "Senior avec l'envie d'un second avis : architecture, positionnement, conférences, prochaine étape",
    ],
    mentorApply: ["Pour en profiter, écrivez-moi avec ", "quelque chose à relire", " (un dépôt, un CV, des slides) et quelques lignes sur ce que vous aimeriez retirer de cette heure."],
    mentorNote: "Si le créneau du trimestre est déjà pris, je vous dirai quand le prochain se libère. Vous ne faites pas partie de ces groupes ? Ma boîte mail reste ouverte ; cette heure est simplement réservée.",
    mentorSubject: "Demande de mentorat",
    contactIntro: ["Pour des opportunités, des invitations à intervenir, des collaborations ou tout ce qui touche à l'open source, écrivez-moi ci-dessous, ou directement à "],
    contactIntroNoForm: "Pour des opportunités, des invitations à intervenir, des collaborations ou tout ce qui touche à l'open source, écrivez-moi à ",
    fName: "Nom", fEmail: "E-mail", fMsg: "Message",
    eName: "Veuillez saisir votre nom.", eEmail: "Veuillez saisir une adresse e-mail valide.", eMsg: "Veuillez écrire quelques mots sur votre demande.",
    sent: "Merci, votre message a bien été envoyé. Je réponds généralement sous deux jours ouvrés.",
    send: "Envoyer", sending: "Envoi…",
    sendFail: "Désolée, le message n'a pas pu être envoyé. Réessayez, ou écrivez-moi directement à",
    formNote: ["Votre message m'arrive directement et n'est jamais partagé. Voir la ", "politique de confidentialité", "."],
    colophon: "Conçu et développé par Cristina DeLisle. Fait avec React. Sans cookies, sans traceurs.",
    legalLink: "Mentions légales & confidentialité",
    blogTitle: "Fragments de pensée",
    blogSub: "Des notes sur l'open source, l'apprentissage, les communautés, et parfois une démo ou une vidéo, écrites pour durer plus longtemps qu'un fil d'actualité.",
    filterPh: "Filtrer les articles…", all: "tout",
    empty: "Aucun résultat. Essayez de retirer le filtre ou le tag.",
    backHome: "← Cristina DeLisle", backPosts: "← Tous les articles", notFound: "Article introuvable.",
    toTop: "Retour en haut", skip: "Aller au contenu",
    themeToLight: "Passer en mode clair", themeToDark: "Passer en mode sombre",
  },
};

type UIStrings = (typeof UI)[Lang];
interface UIProps { ui: UIStrings }
interface PageProps extends UIProps { lang: Lang }

/* ---------- About ----------
   Paragraphs are arrays of segments; { hl: "..." } renders highlighted. */
const ABOUT = {
  en: [
    [
      "I'm a software engineer in R&D at CJDNS. I build ",
      { hl: "full-stack products and backend services" },
      ", from APIs and data pipelines to the interfaces people use every day, with a focus on ",
      { hl: "AI tools people can inspect, self-host, and trust" },
      ". Right now I'm building ", { hl: "Calvad" }, ", an open-source tool that extracts information from documents with AI, checks every answer against its source, and runs entirely on your own hardware.",
    ],
    [
      "I enjoy the whole path from idea to production: understanding the problem, designing the system, shipping it, and checking how it holds up in real use. I'm also interested in ",
      { hl: "applied AI" },
      " and how to make it reliable. I trained in computer science at ",
      { hl: "Georgia Tech (MSc, specialised in AI)" },
      " and the University of London (BSc).",
    ],
    [
      "I live in the Normandy countryside with my husband and our two children, and work remotely, with regular time in Paris and around Normandy when meeting in person matters. I work in ",
      { hl: "English and French" },
      ".",
    ],
    [
      "Outside working hours, you'll likely find me at a hackathon or volunteering to review someone's CV over coffee. The part of this work I love most is collaborating with people. I learn something from everyone I build with, and the mentorship hour below is my way of giving some of that back.",
    ],
  ],
  fr: [
    [
      "Je suis ingénieure logiciel en R&D chez CJDNS. Je construis des ",
      { hl: "produits full stack et des services backend" },
      ", des API et pipelines de données jusqu'aux interfaces utilisées au quotidien, avec une attention particulière aux ",
      { hl: "outils d'IA que l'on peut inspecter, auto-héberger, et auxquels on peut faire confiance" },
      ". En ce moment, je construis ", { hl: "Calvad" }, ", un outil open source qui extrait des informations de documents grâce à l'IA, vérifie chaque réponse dans sa source, et tourne entièrement sur votre propre matériel.",
    ],
    [
      "J'aime tout le chemin de l'idée à la production : comprendre le problème, concevoir le système, le livrer, et vérifier qu'il tient en conditions réelles. Je m'intéresse aussi à l'",
      { hl: "IA appliquée" },
      " et à la manière de la rendre fiable. Je me suis formée en informatique à ",
      { hl: "Georgia Tech (MSc, spécialisé en IA)" },
      " et à l'Université de Londres (BSc).",
    ],
    [
      "Je vis dans la campagne normande avec mon mari et nos deux enfants, et je travaille à distance, avec des déplacements réguliers à Paris et en Normandie quand une rencontre en personne s'impose. Je travaille en ",
      { hl: "français et en anglais" },
      ".",
    ],
    [
      "En dehors des heures de travail, vous me trouverez sans doute à un hackathon ou en train de relire le CV de quelqu'un autour d'un café. Ce que je préfère dans ce métier, c'est la collaboration : j'apprends quelque chose de chaque personne avec qui je construis, et l'heure de mentorat ci-dessous est ma façon de rendre un peu de tout cela.",
    ],
  ],
};

/* ============================================================
   STYLES (same design system as v6)
   ============================================================ */

const css = `
/* Fonts are self-hosted via @fontsource imports in main.jsx (GDPR-clean). */

:root {
  --bg: #16101E; --bg-soft: #1F1730;
  --card: rgba(185,156,232,0.05); --card-line: rgba(185,156,232,0.14);
  --text: #EDE7F8; --text-dim: #9E93B8;
  --wisteria: #B99CE8; --wisteria-strong: #D3BDF7; --wisteria-deep: #7C5FA8;
  --gold: #E8C07D; --shimmer-peak: #FFF6E8; --max: 1240px;
}
.site[data-theme="light"] {
  --bg: #F7F4FA; --bg-soft: #FFFFFF;
  --card: rgba(124,95,168,0.06); --card-line: rgba(124,95,168,0.2);
  --text: #241A2E; --text-dim: #5F5470;
  --wisteria: #7C5FA8; --wisteria-strong: #52396F; --wisteria-deep: #A98FCE;
  --gold: #A6791F; --shimmer-peak: #3A2C52;
}
.site[data-theme="light"] .silk-vignette {
  background: radial-gradient(1200px 800px at 22% 18%, rgba(247,244,250,0.5), transparent 70%), linear-gradient(180deg, rgba(247,244,250,0.3), rgba(247,244,250,0.7));
}
.site[data-theme="light"] .card.featured {
  border-color: rgba(166,121,31,0.45); background: rgba(166,121,31,0.05);
}
* { box-sizing: border-box; margin: 0; padding: 0; }
html { scroll-behavior: smooth; }
body { background: var(--bg); }
.site ::selection { background: var(--wisteria); color: #FFFFFF; }
.site { font-family: 'Inter', system-ui, sans-serif; background: var(--bg); color: var(--text-dim); font-size: 16px; line-height: 1.65; min-height: 100vh; }
a { color: inherit; }
:focus-visible { outline: 2px solid var(--gold); outline-offset: 3px; border-radius: 3px; }
main:focus { outline: none; }
.skip-link { position: absolute; left: -9999px; top: 0; z-index: 200; background: var(--gold); color: var(--bg); padding: 10px 18px; font-weight: 600; border-radius: 0 0 10px 0; }
.skip-link:focus { left: 0; }
.silk { position: fixed; inset: 0; z-index: 0; display: block; width: 100%; height: 100%; }
.silk-vignette { pointer-events: none; position: fixed; inset: 0; z-index: 0;
  background: radial-gradient(1200px 800px at 22% 18%, rgba(22,16,30,0.5), transparent 70%), linear-gradient(180deg, rgba(22,16,30,0.35), rgba(22,16,30,0.68)); }

.lang-switch {
  position: fixed; top: 20px; right: 24px; z-index: 80;
  display: flex; gap: 2px; padding: 3px;
  background: var(--bg-soft); border: 1px solid var(--card-line); border-radius: 999px;
}
.lang-switch button {
  font-family: 'IBM Plex Mono', monospace; font-size: 12px; letter-spacing: 0.1em;
  background: none; border: none; color: var(--text-dim);
  padding: 6px 12px; border-radius: 999px; cursor: pointer;
  transition: background .2s, color .2s;
}
.lang-switch button.on { background: var(--wisteria); color: var(--bg); }
.lang-switch button:not(.on):hover { color: var(--text); }
.lang-switch .theme-btn { display: grid; place-items: center; padding: 6px 10px; }
.lang-switch .theme-btn svg { width: 15px; height: 15px; display: block; }

.layout { position: relative; z-index: 1; max-width: var(--max); margin: 0 auto; display: grid; grid-template-columns: 440px 1fr; gap: 40px; padding: 0 48px; }
.page { position: relative; z-index: 1; max-width: 880px; margin: 0 auto; padding: 72px 28px 90px; }

.sidebar { position: sticky; top: 0; height: 100vh; display: flex; flex-direction: column; justify-content: space-between; padding: 88px 40px 88px 0; }
.portrait-wrap { position: relative; width: 132px; height: 132px; margin-bottom: 30px; animation: bob 7s ease-in-out infinite; }
@keyframes bob { 0%,100% { transform: translateY(0); } 50% { transform: translateY(-6px); } }
.portrait-ring, .portrait-ring-glow { position: absolute; inset: -7px; border-radius: 50%;
  background: conic-gradient(#D3BDF7, #7C5FA8, #2E2145, #E8C07D, #B99CE8, #D3BDF7); animation: spin360 11s linear infinite; }
.portrait-ring-glow { filter: blur(16px); opacity: 0.5; animation-duration: 17s; animation-direction: reverse; }
@keyframes spin360 { to { transform: rotate(360deg); } }
.portrait { position: absolute; inset: 0; border-radius: 50%; overflow: hidden; border: 4px solid var(--bg); }
.portrait img { width: 100%; height: 100%; object-fit: cover; display: block; }
.sidebar h1 { font-family: 'Sora', sans-serif; font-weight: 700; font-size: clamp(36px, 3.4vw, 46px); letter-spacing: -0.03em; color: var(--text); line-height: 1.05; }
.sidebar h1 a { text-decoration: none;
  background: linear-gradient(110deg, var(--text) 25%, var(--wisteria-strong) 42%, var(--shimmer-peak) 50%, var(--wisteria-strong) 58%, var(--text) 75%);
  background-size: 260% 100%; -webkit-background-clip: text; background-clip: text; color: transparent;
  animation: shimmer 8s ease-in-out infinite; }
@keyframes shimmer { 0%,100% { background-position: 100% 0; } 50% { background-position: 0% 0; } }
.sidebar .role { font-family: 'Sora', sans-serif; font-weight: 500; font-size: 18px; color: var(--text); margin-top: 12px; }
.sidebar .tagline { margin-top: 12px; max-width: 34ch; }
.sidebar .tagline strong { color: var(--wisteria); font-weight: 500; }
.side-nav { margin-top: 44px; }
.side-nav ul { list-style: none; }
.side-nav a { display: inline-flex; align-items: center; gap: 14px; padding: 8px 0; text-decoration: none;
  font-family: 'IBM Plex Mono', monospace; font-size: 12.5px; letter-spacing: 0.18em; text-transform: uppercase;
  color: var(--text-dim); transition: color .25s ease; }
.side-nav a .line { width: 32px; height: 1px; background: var(--text-dim); transition: width .3s ease, background .3s ease; }
.side-nav a:hover, .side-nav a.active { color: var(--text); }
.side-nav a:hover .line, .side-nav a.active .line { width: 64px; background: var(--gold); }
.side-foot { display: flex; flex-direction: column; gap: 20px; }
.socials { display: flex; gap: 20px; list-style: none; }
.socials a { color: var(--text-dim); transition: color .25s ease, transform .25s ease; display: block; }
.socials a:hover { color: var(--wisteria-strong); transform: translateY(-3px); }
.socials svg { width: 22px; height: 22px; display: block; }
.avail { display: inline-flex; align-items: center; gap: 10px; font-size: 13.5px; }
.cv-link {
  align-self: flex-start; font-size: 13.5px; font-weight: 600;
  color: var(--text); text-decoration: none;
  border-bottom: 1px solid var(--wisteria-deep);
  transition: color .2s ease, border-color .2s ease;
}
.cv-link:hover { color: var(--gold); border-color: var(--gold); }
.avail .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--gold); box-shadow: 0 0 10px rgba(232,192,125,0.9); animation: pulse 2.4s ease-in-out infinite; }
@keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.45; } }

.content { padding: 88px 0; }
.section { padding-bottom: 100px; scroll-margin-top: 80px; }
.sec-title { font-family: 'IBM Plex Mono', monospace; font-size: 12px; letter-spacing: 0.22em; text-transform: uppercase; color: var(--gold); margin-bottom: 26px; }
.about p { max-width: 60ch; }
.about p + p { margin-top: 16px; }
.hl { color: var(--text); font-weight: 500; }
a.inline { color: var(--text); font-weight: 500; text-decoration: none; border-bottom: 1px solid var(--wisteria-deep); transition: color .2s, border-color .2s; }
a.inline:hover { color: var(--wisteria-strong); border-color: var(--wisteria-strong); }

.hover-list { display: flex; flex-direction: column; }
@media (hover: hover) { .hover-list:hover > * { opacity: 0.45; } .hover-list > *:hover { opacity: 1 !important; } }
.hover-list > * { transition: opacity .3s ease, background .3s ease, transform .3s ease; }
.card { display: grid; grid-template-columns: 110px 1fr; gap: 22px; padding: 24px; border-radius: 14px; text-decoration: none; border: 1px solid transparent; }
.card:hover { background: var(--card); border-color: var(--card-line); transform: translateY(-2px); }
.card .aside { font-family: 'IBM Plex Mono', monospace; font-size: 12px; letter-spacing: 0.08em; text-transform: uppercase; padding-top: 4px; }
.card h3 { font-family: 'Sora', sans-serif; font-weight: 600; font-size: 18px; color: var(--text); }
.card:hover h3 { color: var(--wisteria-strong); }
.card .role { font-size: 13.5px; color: var(--wisteria); margin-top: 2px; }
.card p.blurb { margin-top: 10px; font-size: 14.5px; }
.pills { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 14px; list-style: none; }
.pills li, .tag-chip { font-family: 'IBM Plex Mono', monospace; font-size: 12px; color: var(--wisteria-strong);
  background: rgba(185,156,232,0.1); border: 1px solid rgba(185,156,232,0.18); padding: 5px 12px; border-radius: 999px; }
.card-links { display: flex; gap: 18px; margin-top: 14px; }
.card-links a { font-size: 13.5px; font-weight: 600; color: var(--text); text-decoration: none; border-bottom: 1px solid var(--wisteria-deep); transition: color .2s, border-color .2s; }
.card-links a:hover { color: var(--gold); border-color: var(--gold); }

/* expandable abstract on talk cards */
.abstract { margin-top: 12px; }
.abstract summary {
  cursor: pointer; list-style: none; display: inline-flex; align-items: center; gap: 7px;
  font-family: 'IBM Plex Mono', monospace; font-size: 12px;
  letter-spacing: 0.1em; text-transform: uppercase; color: var(--gold);
}
.abstract summary::-webkit-details-marker { display: none; }
.abstract summary::before { content: "+"; font-size: 14px; transition: transform .2s ease; }
.abstract[open] summary::before { content: "–"; }
.abstract p { margin-top: 10px; font-size: 14.5px; max-width: 62ch; }

/* prominent talk link */
.talk-cta {
  display: inline-flex; align-items: center; gap: 8px; margin-top: 16px;
  font-family: 'Sora', sans-serif; font-weight: 600; font-size: 13.5px;
  color: var(--gold); text-decoration: none;
  border: 1px solid rgba(232,192,125,0.5); border-radius: 999px;
  padding: 9px 16px; transition: background .2s ease, color .2s ease, transform .2s ease;
}
.talk-cta:hover { background: var(--gold); color: var(--bg); transform: translateY(-2px); }

/* show earlier talks */
.show-earlier {
  display: inline-flex; align-items: center; gap: 10px; margin-top: 10px; margin-left: 24px;
  font-family: 'IBM Plex Mono', monospace; font-size: 12.5px;
  letter-spacing: 0.14em; text-transform: uppercase;
  background: none; border: none; color: var(--text-dim); cursor: pointer;
  transition: color .2s ease;
}
.show-earlier::before { content: ""; width: 32px; height: 1px; background: var(--text-dim); transition: width .3s ease, background .3s ease; }
.show-earlier:hover { color: var(--gold); }
.show-earlier:hover::before { width: 56px; background: var(--gold); }
@media (max-width: 1024px) { .show-earlier { margin-left: 0; } }

.card.featured { border: 1px dashed rgba(232,192,125,0.4); background: rgba(232,192,125,0.04); }
.card.featured .aside { color: var(--gold); }
.card.featured h3::after { content: " ✦"; color: var(--gold); }
@keyframes glowpulse { 0%,100% { box-shadow: 0 0 0 rgba(232,192,125,0); } 50% { box-shadow: 0 0 28px -8px rgba(232,192,125,0.35); } }
.card.featured { animation: glowpulse 4.5s ease-in-out infinite; }

.mentor-card { position: relative; border-radius: 18px; padding: 1px; overflow: hidden;
  background: linear-gradient(120deg, rgba(232,192,125,0.55), rgba(185,156,232,0.15) 45%, rgba(185,156,232,0.4)); }
.mentor-inner { border-radius: 17px; background: var(--bg-soft); padding: 34px; }
.mentor-inner h3 { font-family: 'Sora', sans-serif; font-weight: 600; font-size: 20px; color: var(--text); }
.mentor-inner p { margin-top: 10px; max-width: 62ch; }
.mentor-inner ul { margin: 14px 0 0 18px; }
.mentor-inner li { margin-top: 6px; font-size: 14.5px; }
.mentor-mail { margin-top: 22px; }
/* Long email addresses may wrap on narrow phones instead of widening the page. */
.mentor-mail .btn { white-space: normal; overflow-wrap: anywhere; max-width: 100%; text-align: center; }
a.inline { overflow-wrap: anywhere; }
.btn { display: inline-flex; align-items: center; gap: 9px; white-space: nowrap;
  font-family: 'Sora', sans-serif; font-weight: 600; font-size: 15px;
  background: var(--gold); color: var(--bg); padding: 13px 22px; border-radius: 12px; text-decoration: none;
  border: none; cursor: pointer; transition: filter .25s ease, transform .25s ease; }
.btn:hover { filter: brightness(1.1); transform: translateY(-2px); }
.btn.violet { background: var(--wisteria); }
.btn[disabled] { opacity: .6; cursor: default; transform: none; }
.small-note { font-size: 13px; margin-top: 12px; }

.topbar { display: flex; align-items: center; justify-content: space-between; margin-bottom: 40px; }
.back { font-family: 'IBM Plex Mono', monospace; font-size: 13px; letter-spacing: 0.12em; text-transform: uppercase; text-decoration: none; color: var(--text-dim); }
.back:hover { color: var(--gold); }
.page h1.page-title { font-family: 'Sora', sans-serif; font-weight: 700; font-size: clamp(32px, 5vw, 44px); color: var(--text); letter-spacing: -0.02em; }
.page .page-sub { margin-top: 10px; max-width: 60ch; }
.blog-search { position: relative; max-width: 380px; margin-top: 26px; }
.blog-search input { width: 100%; font: inherit; font-size: 15px; color: var(--text);
  background: var(--card); border: 1px solid var(--card-line); border-radius: 999px; padding: 11px 18px 11px 42px;
  transition: border-color .2s, box-shadow .2s; }
.blog-search input:focus { outline: none; border-color: var(--wisteria); box-shadow: 0 0 0 4px rgba(185,156,232,0.15); }
.blog-search svg { position: absolute; left: 15px; top: 50%; transform: translateY(-50%); width: 17px; height: 17px; color: var(--text-dim); pointer-events: none; }
.tag-row { display: flex; flex-wrap: wrap; gap: 8px; margin: 22px 0 36px; }
.tag-chip { cursor: pointer; background: transparent; transition: background .2s, color .2s, border-color .2s; }
.tag-chip:hover { border-color: var(--wisteria); }
.tag-chip.on { background: var(--wisteria); color: var(--bg); border-color: var(--wisteria); }
.tag-chip.gold { color: var(--gold); border-color: rgba(232,192,125,0.35); }
.tag-chip.gold.on { background: var(--gold); color: var(--bg); border-color: var(--gold); }
.post-card { display: block; padding: 26px; border-radius: 14px; text-decoration: none;
  border: 1px solid var(--card-line); background: var(--card); margin-bottom: 18px;
  transition: transform .3s ease, border-color .3s ease; }
.post-card:hover { transform: translateY(-3px); border-color: var(--wisteria-deep); }
.post-card .pmeta { font-family: 'IBM Plex Mono', monospace; font-size: 12px; }
.post-card h2 { font-family: 'Sora', sans-serif; font-weight: 600; font-size: 21px; color: var(--text); margin: 8px 0; }
.post-card:hover h2 { color: var(--wisteria-strong); }
.post-card p { font-size: 14.5px; }
.post-card .pills { margin-top: 14px; }
.article h1 { font-family: 'Sora', sans-serif; font-weight: 700; font-size: clamp(28px, 4.6vw, 40px); color: var(--text); letter-spacing: -0.02em; line-height: 1.15; }
.article .pmeta { font-family: 'IBM Plex Mono', monospace; font-size: 13px; margin-top: 14px; }
.article .pills { margin: 18px 0 34px; }
.article p { margin-bottom: 18px; max-width: 68ch; font-size: 16.5px; }
.article h2 { font-family: 'Sora', sans-serif; font-weight: 600; font-size: 22px; color: var(--text); margin: 34px 0 14px; }

.form { margin-top: 30px; max-width: 560px; }
.field { margin-bottom: 20px; }
.field label { display: block; font-family: 'IBM Plex Mono', monospace; font-size: 12px; letter-spacing: 0.14em; text-transform: uppercase; margin-bottom: 8px; }
.field input, .field textarea { width: 100%; font: inherit; font-size: 15px; color: var(--text);
  background: var(--card); border: 1px solid var(--card-line); border-radius: 12px; padding: 13px 16px;
  transition: border-color .2s, box-shadow .2s; }
.field input:focus, .field textarea:focus { outline: none; border-color: var(--wisteria); box-shadow: 0 0 0 4px rgba(185,156,232,0.15); }
.field .err { color: #F0A7A0; font-size: 13px; margin-top: 6px; }
.field input[aria-invalid="true"], .field textarea[aria-invalid="true"] { border-color: #F0A7A0; }
.form-ok { background: rgba(232,192,125,0.1); border: 1px solid rgba(232,192,125,0.4); color: var(--gold); padding: 16px 18px; border-radius: 12px; }
.hp { position: absolute; left: -9999px; height: 0; overflow: hidden; }
.colophon { font-size: 13px; padding-bottom: 60px; max-width: 60ch; }
.colophon a { color: var(--text); text-decoration: none; border-bottom: 1px solid var(--wisteria-deep); }
.colophon a:hover { color: var(--gold); }

.to-top { position: fixed; right: 26px; bottom: 26px; z-index: 60; width: 48px; height: 48px; border-radius: 50%;
  background: var(--bg-soft); color: var(--wisteria-strong); border: 1px solid var(--card-line); cursor: pointer;
  font-size: 20px; line-height: 1; display: grid; place-items: center;
  opacity: 0; transform: translateY(12px); pointer-events: none;
  transition: opacity .3s ease, transform .3s ease, color .2s ease, border-color .2s ease; }
.to-top.show { opacity: 1; transform: none; pointer-events: auto; }
.to-top:hover { color: var(--gold); border-color: var(--gold); }

.legal h2 { font-family: 'Sora', sans-serif; font-weight: 600; font-size: 20px; color: var(--text); margin: 32px 0 10px; }
.legal p { margin-bottom: 12px; max-width: 68ch; }

@media (max-width: 1024px) {
  .layout { grid-template-columns: 1fr; gap: 0; padding: 0 28px; }
  .sidebar { position: static; height: auto; padding: 84px 0 32px; justify-content: flex-start; gap: 30px; }
  .side-nav { display: none; }
  .content { padding-top: 0; }
  .card { grid-template-columns: 1fr; gap: 8px; padding: 20px 0; }
  .card:hover { background: transparent; border-color: transparent; transform: none; }
  .card.featured { padding: 20px; }
}
@media (prefers-reduced-motion: reduce) {
  html { scroll-behavior: auto; }
  .portrait-wrap, .portrait-ring, .portrait-ring-glow, .avail .dot, .card.featured { animation: none; }
  .sidebar h1 a { animation: none; }
  [class] { transition: none !important; }
  .hover-list:hover > * { opacity: 1; }
}
`;

/* ============================================================
   HOOKS
   ============================================================ */

function useHashRoute() {
  const get = () => (window.location.hash.startsWith("#/") ? window.location.hash : "#/");
  const [route, setRoute] = useState(get);
  useEffect(() => {
    const f = () => setRoute(get());
    window.addEventListener("hashchange", f);
    return () => window.removeEventListener("hashchange", f);
  }, []);
  useEffect(() => {
    window.scrollTo(0, 0);
    // move focus to the main region so screen readers announce the new page
    requestAnimationFrame(() => {
      const main = document.getElementById("main");
      if (main) main.focus({ preventScroll: true });
    });
  }, [route]);
  return route;
}

function useScrollSpy(ids: string[], enabled: boolean) {
  const [active, setActive] = useState(ids[0]);
  useEffect(() => {
    if (!enabled) return;
    const obs = new IntersectionObserver(
      (entries) => { for (const e of entries) if (e.isIntersecting) setActive(e.target.id); },
      { rootMargin: "-30% 0px -60% 0px" }
    );
    ids.forEach((id) => { const el = document.getElementById(id); if (el) obs.observe(el); });
    return () => obs.disconnect();
  }, [ids, enabled]);
  return active;
}

/* Calm field: slow horizontal contour lines with drifting thin
   circles. Lines bow gently away from the pointer; circles have a
   light parallax. Fully redrawn each frame (crisp, no trails).
   Static single frame under prefers-reduced-motion. */
function useCalmField(canvasRef: RefObject<HTMLCanvasElement>, theme: Theme) {
  useEffect(() => {
    const maybeCanvas = canvasRef.current;
    const maybeCtx = maybeCanvas?.getContext("2d");
    if (!maybeCanvas || !maybeCtx) return;
    const canvas: HTMLCanvasElement = maybeCanvas;
    const ctx: CanvasRenderingContext2D = maybeCtx;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const coarse = window.matchMedia("(pointer: coarse)").matches;
    const dark = theme !== "light";
    const BG = dark ? "#16101E" : "#F7F4FA";
    const lineCol = (a: number) => (dark ? `hsla(266, 45%, 76%, ${a})` : `hsla(268, 38%, 38%, ${a})`);
    const goldCol = (a: number) => (dark ? `hsla(40, 62%, 70%, ${a})` : `hsla(40, 70%, 34%, ${a})`);

    let raf = 0, w = 0, h = 0;
    let lines: {
      y: number; amp1: number; amp2: number; f1: number; f2: number;
      s1: number; s2: number; phase: number; alpha: number;
    }[] = [];
    let circles: {
      x: number; y: number; r: number; vx: number; vy: number;
      depth: number; gold: boolean; alpha: number; lw: number;
    }[] = [];
    const pointer = { x: -1e4, y: -1e4 };
    const rand = (a: number, b: number) => a + Math.random() * (b - a);

    function build() {
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      w = canvas.clientWidth; h = canvas.clientHeight;
      canvas.width = w * dpr; canvas.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      const n = Math.max(10, Math.round(h / 90));
      lines = Array.from({ length: n }, (_, i) => ({
        y: (h / (n + 1)) * (i + 1),
        amp1: rand(10, 26), amp2: rand(4, 10),
        f1: rand(0.002, 0.004), f2: rand(0.007, 0.011),
        s1: rand(0.00008, 0.00016) * (Math.random() < 0.5 ? 1 : -1),
        s2: rand(0.00018, 0.0003),
        phase: rand(0, Math.PI * 2),
        alpha: rand(0.05, 0.11),
      }));

      circles = Array.from({ length: 9 }, () => ({
        x: rand(0, w), y: rand(0, h), r: rand(30, 170),
        vx: rand(-0.05, 0.05), vy: rand(-0.035, 0.035),
        depth: rand(0.3, 1),
        gold: Math.random() < 0.22,
        alpha: rand(0.05, 0.12),
        lw: rand(0.7, 1.4),
      }));
    }

    function draw(t: number) {
      ctx.fillStyle = BG;
      ctx.fillRect(0, 0, w, h);

      // circles (behind lines), slight pointer parallax
      for (const c of circles) {
        if (!reduced) { c.x += c.vx; c.y += c.vy; }
        if (c.x < -c.r) c.x = w + c.r; if (c.x > w + c.r) c.x = -c.r;
        if (c.y < -c.r) c.y = h + c.r; if (c.y > h + c.r) c.y = -c.r;
        const px = pointer.x > -1e3 ? (pointer.x - w / 2) * 0.012 * c.depth : 0;
        const py = pointer.y > -1e3 ? (pointer.y - h / 2) * 0.012 * c.depth : 0;
        ctx.beginPath();
        ctx.arc(c.x - px, c.y - py, c.r, 0, Math.PI * 2);
        ctx.strokeStyle = c.gold ? goldCol(c.alpha) : lineCol(c.alpha);
        ctx.lineWidth = c.lw;
        ctx.stroke();
      }

      // contour lines, bowing gently away from the pointer
      const sigma = 130, sigma2 = 2 * sigma * sigma;
      for (const L of lines) {
        ctx.beginPath();
        for (let x = -10; x <= w + 10; x += 7) {
          let y = L.y
            + Math.sin(x * L.f1 + t * L.s1 + L.phase) * L.amp1
            + Math.sin(x * L.f2 - t * L.s2) * L.amp2;
          if (pointer.x > -1e3) {
            const dx = x - pointer.x;
            const g = Math.exp(-(dx * dx) / sigma2);
            const dy = y - pointer.y;
            const dist = Math.abs(dy);
            if (dist < 160) {
              const push = (1 - dist / 160) * g * 22;
              y += (dy >= 0 ? 1 : -1) * push;
            }
          }
          if (x <= -10) ctx.moveTo(x, y); else ctx.lineTo(x, y);
        }
        ctx.strokeStyle = lineCol(L.alpha);
        ctx.lineWidth = 1;
        ctx.stroke();
      }

      if (!reduced) raf = requestAnimationFrame(draw);
    }

    function onMove(e: PointerEvent) { pointer.x = e.clientX; pointer.y = e.clientY; }
    function onLeave() { pointer.x = -1e4; pointer.y = -1e4; }
    function onResize() { build(); if (reduced) draw(0); }

    build();
    if (reduced) draw(0); else raf = requestAnimationFrame(draw);
    if (!coarse) {
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerleave", onLeave);
    }
    window.addEventListener("resize", onResize);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerleave", onLeave);
      window.removeEventListener("resize", onResize);
    };
  }, [canvasRef, theme]);
}

/* ============================================================
   ICONS
   ============================================================ */

const GitHubIcon = () => (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.11.79-.25.79-.55v-2.17c-3.2.7-3.87-1.36-3.87-1.36-.53-1.33-1.28-1.69-1.28-1.69-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.19 1.76 1.19 1.03 1.75 2.69 1.25 3.34.95.1-.74.4-1.25.72-1.53-2.55-.29-5.23-1.28-5.23-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.18 1.18a11.1 11.1 0 0 1 5.78 0c2.21-1.49 3.18-1.18 3.18-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.69 5.38-5.25 5.67.41.35.77 1.05.77 2.12v3.15c0 .3.21.66.8.55A11.5 11.5 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5z"/></svg>
);
const LinkedInIcon = () => (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M20.45 20.45h-3.55v-5.57c0-1.33-.03-3.04-1.85-3.04-1.86 0-2.14 1.45-2.14 2.94v5.67H9.35V9h3.41v1.56h.05c.48-.9 1.64-1.85 3.37-1.85 3.6 0 4.27 2.37 4.27 5.46v6.28zM5.34 7.43a2.06 2.06 0 1 1 0-4.12 2.06 2.06 0 0 1 0 4.12zM7.11 20.45H3.56V9h3.55v11.45z"/></svg>
);
const MastodonIcon = () => (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M21.58 13.9c-.29 1.5-2.61 3.14-5.28 3.46-1.39.17-2.76.32-4.22.25-2.39-.11-4.27-.57-4.27-.57 0 .23.01.45.04.66.31 2.36 2.34 2.5 4.26 2.56 1.94.07 3.67-.48 3.67-.48l.08 1.75s-1.36.73-3.78.87c-1.33.07-2.99-.03-4.92-.55-4.19-1.1-4.91-5.57-5.02-10.09-.03-1.34-.01-2.61-.01-3.67 0-4.62 3.03-5.97 3.03-5.97C6.69 1.42 9.31 1.13 12.03 1.1h.07c2.72.03 5.34.32 6.87 1.02 0 0 3.03 1.35 3.03 5.97 0 0 .04 3.4-.42 5.81zm-3.15-5.45c0-1.14-.29-2.05-.88-2.72-.6-.67-1.39-1.01-2.37-1.01-1.14 0-2 .44-2.57 1.31l-.55.93-.55-.93c-.57-.87-1.43-1.31-2.57-1.31-.98 0-1.77.34-2.37 1.01-.59.67-.88 1.58-.88 2.72v5.58h2.21V8.61c0-1.14.48-1.72 1.44-1.72 1.06 0 1.59.69 1.59 2.04v2.96h2.2V8.93c0-1.35.53-2.04 1.59-2.04.96 0 1.44.58 1.44 1.72v5.42h2.21V8.45z"/></svg>
);

/* ============================================================
   COMPONENTS
   ============================================================ */

interface TopControlsProps extends PageProps {
  setLang: (l: Lang) => void;
  theme: Theme;
  setTheme: (t: Theme) => void;
}

function TopControls({ lang, setLang, theme, setTheme, ui }: TopControlsProps) {
  const dark = theme !== "light";
  return (
    <div className="lang-switch" role="group" aria-label="Language & theme / Langue et thème">
      <button className={lang === "en" ? "on" : ""} onClick={() => setLang("en")} aria-pressed={lang === "en"} lang="en">EN</button>
      <button className={lang === "fr" ? "on" : ""} onClick={() => setLang("fr")} aria-pressed={lang === "fr"} lang="fr">FR</button>
      <button
        className="theme-btn"
        onClick={() => setTheme(dark ? "light" : "dark")}
        aria-label={dark ? ui.themeToLight : ui.themeToDark}
        title={dark ? ui.themeToLight : ui.themeToDark}
      >
        {dark ? (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
            <circle cx="12" cy="12" r="4.5" />
            <path d="M12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M19.1 4.9l-1.8 1.8M6.7 17.3l-1.8 1.8" />
          </svg>
        ) : (
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M20.5 14.6A8.5 8.5 0 0 1 9.4 3.5a8.5 8.5 0 1 0 11.1 11.1z" />
          </svg>
        )}
      </button>
    </div>
  );
}

function ToTop({ ui }: UIProps) {
  const [show, setShow] = useState(false);
  useEffect(() => {
    const f = () => setShow(window.scrollY > 600);
    window.addEventListener("scroll", f, { passive: true });
    f();
    return () => window.removeEventListener("scroll", f);
  }, []);
  const up = () => {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    window.scrollTo({ top: 0, behavior: reduced ? "auto" : "smooth" });
  };
  return (
    <button className={`to-top ${show ? "show" : ""}`} onClick={up} aria-label={ui.toTop}>↑</button>
  );
}

function Sidebar({ active, ui }: UIProps & { active: string }) {
  const nav: [string, string][] = [
    ["about", ui.nav.about], ["projects", ui.nav.projects], ["talks", ui.nav.talks],
    ["mentorship", ui.nav.mentorship], ["contact", ui.nav.contact],
  ];
  return (
    <aside className="sidebar">
      <div>
        <div className="portrait-wrap">
          <span className="portrait-ring-glow" aria-hidden="true" />
          <span className="portrait-ring" aria-hidden="true" />
          <div className="portrait"><img src={PHOTO_URL} alt="Portrait of Cristina DeLisle" /></div>
        </div>
        <h1><a href="#about">Cristina DeLisle</a></h1>
        <p className="role">{ui.role}</p>
        <p className="tagline">{ui.tagline[0]}<strong>{ui.tagline[1]}</strong>{ui.tagline[2]}</p>
        <nav className="side-nav" aria-label="Sections">
          <ul>
            {nav.map(([id, label]) => (
              <li key={id}>
                <a href={`#${id}`} className={active === id ? "active" : ""}>
                  <span className="line" aria-hidden="true" /> {label}
                </a>
              </li>
            ))}
            <li><a href="#/blog"><span className="line" aria-hidden="true" /> {ui.nav.blog}</a></li>
          </ul>
        </nav>
      </div>
      <div className="side-foot">
        <a className="cv-link" href={`mailto:${LINKS.email}?subject=${encodeURIComponent(ui.cvSubject)}`}>{ui.cvLabel}</a>
        <p className="avail"><span className="dot" aria-hidden="true" /> {ui.avail}</p>
        <ul className="socials">
          <li><a href={LINKS.github} target="_blank" rel="noopener noreferrer" aria-label="GitHub"><GitHubIcon /></a></li>
          <li><a href={LINKS.linkedin} target="_blank" rel="noopener noreferrer" aria-label="LinkedIn"><LinkedInIcon /></a></li>
          <li><a href={LINKS.mastodon} target="_blank" rel="me noopener noreferrer" aria-label="Mastodon"><MastodonIcon /></a></li>
        </ul>
      </div>
    </aside>
  );
}

function About({ lang, ui }: PageProps) {
  return (
    <section className="section about" id="about" aria-label={ui.secAbout}>
      <h2 className="sec-title">{ui.secAbout}</h2>
      {ABOUT[lang].map((para, i) => (
        <p key={i}>
          {para.map((seg, j) =>
            typeof seg === "string" ? seg : <span className="hl" key={j}>{seg.hl}</span>
          )}
        </p>
      ))}
    </section>
  );
}

/* The first VISIBLE_PROJECTS show by default; the rest sit behind
   "See more". Order PROJECTS most-important-first. */
const VISIBLE_PROJECTS = 4;

function Projects({ lang, ui }: PageProps) {
  const [showAll, setShowAll] = useState(false);
  const hidden = Math.max(0, PROJECTS.length - VISIBLE_PROJECTS);
  const visible = showAll ? PROJECTS : PROJECTS.slice(0, VISIBLE_PROJECTS);
  return (
    <section className="section" id="projects" aria-label={ui.secProjects}>
      <h2 className="sec-title">{ui.secProjects}</h2>
      <div className="hover-list">
        {visible.map((p, idx) => (
          <article className={`card ${p.featured ? "featured" : ""}`} key={idx}>
            <span className="aside">{t(p.tags[0], lang)}</span>
            <div>
              <h3>{t(p.name, lang)}</h3>
              <p className="role">{t(p.role, lang)}</p>
              <p className="blurb">{t(p.blurb, lang)}</p>
              <ul className="pills" aria-label="Tags">
                {p.tags.map((tag, i) => <li key={i}>{t(tag, lang)}</li>)}
              </ul>
              {p.links.length > 0 && (
                <div className="card-links">
                  {p.links.map((l, i) => (
                    <a key={i} href={l.href} target={l.href.startsWith("#") ? undefined : "_blank"} rel="noopener noreferrer">
                      {t(l.label, lang)}
                    </a>
                  ))}
                </div>
              )}
            </div>
          </article>
        ))}
      </div>
      {hidden > 0 && (
        <button className="show-earlier" onClick={() => setShowAll(!showAll)} aria-expanded={showAll}>
          {showAll ? ui.seeFewerProjects : ui.seeMoreProjects(hidden)}
        </button>
      )}
    </section>
  );
}

/* The 4 most recent talks show by default; the rest sit behind
   "Show earlier talks". Order TALKS newest-first. */
const RECENT_TALKS = 4;

function Talks({ lang, ui }: PageProps) {
  const [showAll, setShowAll] = useState(false);
  const earlier = Math.max(0, TALKS.length - RECENT_TALKS);
  const visible = showAll ? TALKS : TALKS.slice(0, RECENT_TALKS);
  return (
    <section className="section" id="talks" aria-label={ui.secTalks}>
      <h2 className="sec-title">{ui.secTalks}</h2>
      <div className="hover-list">
        {visible.map((tk, i) => (
          <article className="card" key={i}>
            <span className="aside">{tk.year}</span>
            <div>
              <h3>{t(tk.title, lang)}</h3>
              <p className="role">{tk.event}</p>
              <p className="blurb">{t(tk.description, lang)}</p>
              <ul className="pills" aria-label="Talk details">
                <li>{tk.lang}</li>
                <li>{t(tk.theme, lang)}</li>
              </ul>
              {tk.abstract && (
                <details className="abstract">
                  <summary>{lang === "fr" ? "Résumé" : "Abstract"}</summary>
                  <p>{t(tk.abstract, lang)}</p>
                </details>
              )}
              {tk.link && (
                <a className="talk-cta" href={tk.link} target="_blank" rel="noopener noreferrer">
                  {ui.watch}
                </a>
              )}
            </div>
          </article>
        ))}
      </div>
      {earlier > 0 && (
        <button className="show-earlier" onClick={() => setShowAll(!showAll)} aria-expanded={showAll}>
          {showAll ? ui.hideEarlier : ui.showEarlier(earlier)}
        </button>
      )}
      <div className="mentor-card" style={{ marginTop: 34 }}>
        <div className="mentor-inner">
          <h3>{ui.inviteTitle}</h3>
          <p>{ui.inviteTopics}</p>
          <ul>{SPEAKER_TOPICS[lang].map((s) => <li key={s}>{s}</li>)}</ul>
          <p className="mentor-mail">
            <a className="btn violet" href={`mailto:${LINKS.email}?subject=${encodeURIComponent(ui.inviteSubject)}`}>{ui.inviteBtn}</a>
          </p>
        </div>
      </div>
    </section>
  );
}

function Mentorship({ ui }: UIProps) {
  return (
    <section className="section" id="mentorship" aria-label={ui.secMentorship}>
      <h2 className="sec-title">{ui.secMentorship}</h2>
      <div className="mentor-card">
        <div className="mentor-inner">
          <h3>{ui.mentorTitle}</h3>
          <p>
            {ui.mentorIntro[0]}
            <a className="inline" href={LINKS.ellestime} target="_blank" rel="noopener noreferrer">{ui.mentorIntro[1]}</a>
            {ui.mentorIntro[2]}
          </p>
          <ul>{ui.mentorItems.map((m) => <li key={m}>{m}</li>)}</ul>
          <p style={{ marginTop: 14 }}>
            {ui.mentorApply[0]}<span className="hl">{ui.mentorApply[1]}</span>{ui.mentorApply[2]}
          </p>
          <p className="mentor-mail">
            <a className="btn" href={`mailto:${LINKS.email}?subject=${encodeURIComponent(ui.mentorSubject)}`}>{LINKS.email}</a>
          </p>
          <p className="small-note">{ui.mentorNote}</p>
        </div>
      </div>
    </section>
  );
}

type ContactField = "name" | "email" | "message" | "website";
type ContactErrors = Partial<Record<"name" | "email" | "message", string>>;

function Contact({ ui }: UIProps) {
  const [values, setValues] = useState<Record<ContactField, string>>({ name: "", email: "", message: "", website: "" });
  const [errors, setErrors] = useState<ContactErrors>({});
  const [sent, setSent] = useState(false);
  const [sending, setSending] = useState(false);
  const [failed, setFailed] = useState(false);
  const set = (k: ContactField) => (e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
    setValues((v) => ({ ...v, [k]: e.target.value }));

  function validate(): ContactErrors {
    const e: ContactErrors = {};
    if (!values.name.trim()) e.name = ui.eName;
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(values.email)) e.email = ui.eEmail;
    if (values.message.trim().length < 10) e.message = ui.eMsg;
    return e;
  }
  async function submit() {
    if (values.website) return; // honeypot (also enforced server-side)
    const e = validate();
    setErrors(e);
    if (Object.keys(e).length) return;
    setSending(true);
    setFailed(false);
    try {
      const res = await fetch(CONTACT_ENDPOINT, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(values),
      });
      if (res.ok) setSent(true);
      else setFailed(true);
    } catch {
      setFailed(true);
    } finally {
      setSending(false);
    }
  }

  return (
    <section className="section" id="contact" aria-label={ui.secContact}>
      <h2 className="sec-title">{ui.secContact}</h2>
      <p style={{ maxWidth: "60ch" }}>
        {CONTACT_FORM_ON ? ui.contactIntro[0] : ui.contactIntroNoForm}
        <a className="inline" href={`mailto:${LINKS.email}`}>{LINKS.email}</a>.
      </p>
      {CONTACT_FORM_ON && <div className="form">
        {sent ? (
          <p className="form-ok" role="status">{ui.sent}</p>
        ) : (
          <div aria-label={ui.secContact} role="form">
            <div className="field">
              <label htmlFor="cf-name">{ui.fName}</label>
              <input id="cf-name" type="text" autoComplete="name" value={values.name} onChange={set("name")}
                aria-invalid={!!errors.name} aria-describedby={errors.name ? "cf-name-err" : undefined} />
              {errors.name && <p className="err" id="cf-name-err">{errors.name}</p>}
            </div>
            <div className="field">
              <label htmlFor="cf-email">{ui.fEmail}</label>
              <input id="cf-email" type="email" autoComplete="email" inputMode="email" value={values.email} onChange={set("email")}
                aria-invalid={!!errors.email} aria-describedby={errors.email ? "cf-email-err" : undefined} />
              {errors.email && <p className="err" id="cf-email-err">{errors.email}</p>}
            </div>
            <div className="field">
              <label htmlFor="cf-msg">{ui.fMsg}</label>
              <textarea id="cf-msg" rows={5} value={values.message} onChange={set("message")}
                aria-invalid={!!errors.message} aria-describedby={errors.message ? "cf-msg-err" : undefined} />
              {errors.message && <p className="err" id="cf-msg-err">{errors.message}</p>}
            </div>
            <div className="hp" aria-hidden="true">
              <label htmlFor="cf-web">Website</label>
              <input id="cf-web" type="text" tabIndex={-1} autoComplete="off" value={values.website} onChange={set("website")} />
            </div>
            <button className="btn violet" onClick={submit} disabled={sending}>
              {sending ? ui.sending : ui.send}
            </button>
            {failed && (
              <p className="err" role="alert">
                {ui.sendFail} <a className="inline" href={`mailto:${LINKS.email}`}>{LINKS.email}</a>.
              </p>
            )}
            <p className="small-note">
              {ui.formNote[0]}<a className="inline" href="#/legal">{ui.formNote[1]}</a>{ui.formNote[2]}
            </p>
          </div>
        )}
      </div>}
    </section>
  );
}

function Colophon({ ui }: UIProps) {
  return (
    <footer className="colophon">
      <p>
        {ui.colophon} <a href="#/legal">{ui.legalLink}</a>. ©{new Date().getFullYear()}
      </p>
    </footer>
  );
}

const HOME_SECTIONS = ["about", "projects", "talks", "mentorship", "contact"];

function HomePage({ lang, ui }: PageProps) {
  const active = useScrollSpy(HOME_SECTIONS, true);
  return (
    <div className="layout">
      <Sidebar active={active} ui={ui} />
      <main className="content" id="main" tabIndex={-1}>
        <About lang={lang} ui={ui} />
        <Projects lang={lang} ui={ui} />
        <Talks lang={lang} ui={ui} />
        <Mentorship ui={ui} />
        <Contact ui={ui} />
        <Colophon ui={ui} />
      </main>
    </div>
  );
}

/* ============================================================
   BLOG PAGES
   ============================================================ */

function BlogIndex({ lang, ui }: PageProps) {
  const [tag, setTag] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const needle = q.trim().toLowerCase();
  const posts = BLOG_POSTS.filter((p) => {
    if (tag && !p.tags.includes(tag)) return false;
    if (!needle) return true;
    return (
      t(p.title, lang).toLowerCase().includes(needle) ||
      t(p.excerpt, lang).toLowerCase().includes(needle) ||
      p.tags.some((x) => tagLabel(x, lang).toLowerCase().includes(needle))
    );
  });
  return (
    <main className="page" id="main" tabIndex={-1}>
      <div className="topbar"><a className="back" href="#/">{ui.backHome}</a></div>
      <h1 className="page-title">{ui.blogTitle}</h1>
      <p className="page-sub">{ui.blogSub}</p>
      <div className="blog-search">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
          <circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" />
        </svg>
        <input type="search" placeholder={ui.filterPh} aria-label={ui.filterPh}
          value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <div className="tag-row" role="group" aria-label="Tags">
        <button className={`tag-chip gold ${tag === null ? "on" : ""}`} onClick={() => setTag(null)}>{ui.all}</button>
        {BLOG_TAGS.map((x) => (
          <button key={x} className={`tag-chip ${tag === x ? "on" : ""}`} onClick={() => setTag(tag === x ? null : x)}>
            {tagLabel(x, lang)}
          </button>
        ))}
      </div>
      {posts.length === 0 && <p>{ui.empty}</p>}
      {posts.map((p) => (
        <a className="post-card" key={p.slug} href={`#/blog/${p.slug}`}>
          <span className="pmeta">{t(p.date, lang)}</span>
          <h2>{t(p.title, lang)}</h2>
          <p>{t(p.excerpt, lang)}</p>
          <ul className="pills" aria-label="Tags">
            {p.tags.map((x) => <li key={x}>{tagLabel(x, lang)}</li>)}
          </ul>
        </a>
      ))}
    </main>
  );
}

function BlogPost({ slug, lang, ui }: PageProps & { slug: string }) {
  const post = BLOG_POSTS.find((p) => p.slug === slug);
  if (!post) {
    return (
      <main className="page" id="main" tabIndex={-1}>
        <div className="topbar"><a className="back" href="#/blog">{ui.backPosts}</a></div>
        <p>{ui.notFound}</p>
      </main>
    );
  }
  return (
    <main className="page article" id="main" tabIndex={-1}>
      <div className="topbar"><a className="back" href="#/blog">{ui.backPosts}</a></div>
      <h1>{t(post.title, lang)}</h1>
      <p className="pmeta">{t(post.date, lang)}</p>
      <ul className="pills" aria-label="Tags">
        {post.tags.map((x) => <li key={x}>{tagLabel(x, lang)}</li>)}
      </ul>
      {post.body[lang].map((b, i) => (b.t === "h2" ? <h2 key={i}>{b.x}</h2> : <p key={i}>{b.x}</p>))}
    </main>
  );
}

/* ============================================================
   LEGAL — mentions légales (FR) with an English rendering.
   ============================================================ */

const LEGAL = {
  fr: {
    title: "Mentions légales & confidentialité",
    sections: [
      ["Éditeur du site", "Ce site est édité par Cristina DeLisle, à titre personnel. Directrice de la publication : Cristina DeLisle. Contact : hello@cristinadelisle.xyz."],
      ["Hébergement", "OVH Hosting Limited (groupe OVHcloud), Block 3, Harcourt Centre, Harcourt Road, Dublin 2, D02 A339, Irlande. Numéro d'immatriculation : 468585. ovhcloud.com. Les serveurs sont situés au Canada (centre de données de Beauharnois, Québec), pays reconnu par la Commission européenne comme assurant un niveau adéquat de protection des données."],
      ["Propriété intellectuelle", "Sauf mention contraire, les contenus de ce site (textes, visuels, code) sont la propriété de leur auteure. Les projets open source cités sont distribués sous leurs licences respectives."],
      ["Données personnelles (RGPD)", "Le formulaire de contact collecte votre nom, votre adresse e-mail et votre message, dans le seul but de répondre à votre demande (base légale : intérêt légitime). Ces données sont conservées le temps du traitement de votre demande et ne sont ni cédées ni utilisées à d'autres fins. Vous disposez d'un droit d'accès, de rectification et d'effacement, que vous pouvez exercer en écrivant à hello@cristinadelisle.xyz. Vous pouvez également introduire une réclamation auprès de la CNIL (cnil.fr)."],
      ["Cookies et mesure d'audience", "Ce site n'utilise ni cookies ni traceurs. Il mesure sa fréquentation avec son propre outil, sans cookie : pour chaque page consultée, il enregistre uniquement la page, le site d'où vient le visiteur (nom de domaine seulement), la langue de l'interface et le type d'appareil (mobile, tablette ou ordinateur). Aucune adresse IP, aucun identifiant et aucune donnée permettant de reconnaître un visiteur ou de suivre son parcours ne sont conservés. Les données détaillées sont supprimées après 13 mois ; seuls des totaux journaliers anonymes sont conservés au-delà. Les navigateurs qui envoient le signal Do Not Track ou Global Privacy Control ne sont pas mesurés. Ces statistiques ne sont partagées avec personne."],
    ],
  },
  en: {
    title: "Legal notice & privacy",
    sections: [
      ["Site publisher", "This site is published by Cristina DeLisle, in a personal capacity. Publication director: Cristina DeLisle. Contact: hello@cristinadelisle.xyz."],
      ["Hosting", "OVH Hosting Limited (OVHcloud group), Block 3, Harcourt Centre, Harcourt Road, Dublin 2, D02 A339, Ireland. Company number: 468585. ovhcloud.com. The servers are located in Canada (Beauharnois data centre, Quebec), a country recognised by the European Commission as providing an adequate level of data protection."],
      ["Intellectual property", "Unless stated otherwise, the contents of this site (texts, visuals, code) are the property of their author. The open-source projects mentioned are distributed under their respective licences."],
      ["Personal data (GDPR)", "The contact form collects your name, email address and message, solely to answer your request (legal basis: legitimate interest). This data is kept for the time needed to handle your request and is neither shared nor used for other purposes. You have rights of access, rectification and erasure, which you can exercise by writing to hello@cristinadelisle.xyz. You may also lodge a complaint with the CNIL (cnil.fr)."],
      ["Cookies and audience measurement", "This site uses no cookies and no trackers. It measures its traffic with its own cookieless tool: for each page viewed, it records only the page, the website the visitor came from (domain name only), the interface language and the device type (mobile, tablet or desktop). No IP address, no identifier and nothing that could recognise a visitor or follow their path through the site is kept. Detailed records are deleted after 13 months; only anonymous daily totals are kept beyond that. Browsers sending a Do Not Track or Global Privacy Control signal are not measured. These statistics are not shared with anyone."],
    ],
  },
};

function LegalPage({ lang, ui }: PageProps) {
  const L = LEGAL[lang];
  return (
    <main className="page legal" id="main" tabIndex={-1}>
      <div className="topbar"><a className="back" href="#/">{ui.backHome}</a></div>
      <h1 className="page-title">{L.title}</h1>
      {L.sections.map(([h, p]) => (
        <div key={h}><h2>{h}</h2><p>{p}</p></div>
      ))}
    </main>
  );
}

/* ============================================================
   APP
   ============================================================ */

export default function App() {
  const route = useHashRoute();
  const [lang, setLang] = useState<Lang>(() =>
    (navigator.language || "en").toLowerCase().startsWith("fr") ? "fr" : "en"
  );
  const [theme, setTheme] = useState<Theme>(() =>
    window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark"
  );
  const ui = UI[lang];
  useEffect(() => { document.documentElement.lang = lang; }, [lang]);
  // No-op unless built with VITE_ANALYTICS=on (see api.ts).
  useEffect(() => { trackPageview(route, lang); }, [route]);
  useEffect(() => {
    let title = "Cristina DeLisle · " + (lang === "fr" ? "Ingénieure logiciel R&D" : "R&D Software Engineer");
    if (route === "#/blog") title = ui.blogTitle + " · Cristina DeLisle";
    else if (route.startsWith("#/blog/")) {
      const p = BLOG_POSTS.find((x) => x.slug === route.slice("#/blog/".length));
      if (p) title = t(p.title, lang) + " · Cristina DeLisle";
    } else if (route === "#/legal") title = ui.legalLink + " · Cristina DeLisle";
    document.title = title;
  }, [route, lang, ui]);
  useEffect(() => {
    document.body.style.background = theme === "light" ? "#F7F4FA" : "#16101E";
  }, [theme]);

  const silkRef = useRef<HTMLCanvasElement>(null);
  useCalmField(silkRef, theme);

  let page;
  if (route.startsWith("#/blog/")) page = <BlogPost slug={route.slice("#/blog/".length)} lang={lang} ui={ui} />;
  else if (route === "#/blog") page = <BlogIndex lang={lang} ui={ui} />;
  else if (route === "#/legal") page = <LegalPage lang={lang} ui={ui} />;
  else page = <HomePage lang={lang} ui={ui} />;

  return (
    <div className="site" data-theme={theme}>
      <style>{css}</style>
      <a className="skip-link" href="#main">{ui.skip}</a>
      <canvas ref={silkRef} className="silk" aria-hidden="true" />
      <div className="silk-vignette" aria-hidden="true" />
      <TopControls lang={lang} setLang={setLang} theme={theme} setTheme={setTheme} ui={ui} />
      {page}
      <ToTop ui={ui} />
    </div>
  );
}
