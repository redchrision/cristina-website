//! Backend for cristinadelisle.xyz.
//!
//! * `app`      – state, router, middleware (security headers, RED metrics)
//! * `api_v1`   – the public, versioned REST API + OpenAPI document
//! * `auth`     – API keys, scopes, authentication middleware
//! * `quota`    – token-bucket rate limits and daily quotas
//! * `events`   – event ingestion pipeline into ClickHouse
//! * `stats`    – aggregate queries over ClickHouse
//! * `mcp`      – the same operations as MCP tools for AI agents
//! * `contact`  – contact-form backend
//! * `content`  – site content embedded from /content

pub mod api_v1;
pub mod app;
pub mod auth;
pub mod config;
pub mod contact;
pub mod content;
pub mod error;
pub mod events;
pub mod mcp;
pub mod quota;
pub mod stats;
