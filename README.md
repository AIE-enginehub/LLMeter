# LLMeter

[English](README.md) | [中文](README_zh.md)

An AI API Proxy Gateway — Unify, proxy, and forward APIs from various AI providers. Features multi-organization management, API Key distribution, usage statistics, request log tracking, and a credit billing system.

## 🌟 Features

- **Unified Proxy**: Compatible with standard AI API protocols like OpenAI, Gemini, and Anthropic. Clients only need to change the `base_url` for seamless integration.
- **Multi-Organization**: Support creating independent organizations for different teams or clients. Each organization has its own API Keys and model configurations.
- **Credit System**: Built-in credit billing system. Customize deduction rates for different token types (prompt, completion, cached). Automatically blocks requests when credits are exhausted.
- **API Key Management**: Securely generate and distribute API Keys (with `gc-` prefix). Stores SHA-256 hashes; the full key is only visible upon creation.
- **Model Routing**: Supports wildcard matching (e.g., `gpt-*`, `gemini-*`) and automatically routes requests to the corresponding provider based on priority.
- **Usage Statistics**: Real-time tracking of token usage (prompt / completion / cached) per request. Aggregates data by organization, model, and date.
- **Request Logs**: Fully records request and response payloads. Supports pagination and multi-condition filtering.
- **Admin Dashboard**: Modern built-in Web UI (supports English and Chinese) for managing organizations, keys, models, logs, and system settings.
- **Streaming Support**: Fully supports SSE (Server-Sent Events) streaming responses, forwarding chunks in real-time.
- **High Performance**: Built with Rust (Axum + Tokio) for extremely low memory footprint and high concurrency.

## 📸 Screenshots

### Overview

![Overview](docs/images_en/Overview.png)

### Organizations

![Organizations](docs/images_en/Org_List.png)

### Logs

![Log List](docs/images_en/Log_List.png)

![Log Detail](docs/images_en/Log_Detail.png)

### Usage Statistics

![Usage Statistics](docs/images_en/Usage.png)

### Settings

![Settings](docs/images_en/Settings.png)

## 🚀 Quick Start (Docker Compose)

The easiest way to get started is using Docker Compose.

```bash
# 1. Clone the repository
git clone https://github.com/AIE-enginehub/LLMeter.git
cd LLMeter

# 2. Start the services (PostgreSQL + App)
docker compose up -d

# 3. View logs
docker compose logs -f app
```

After starting, access the admin dashboard at `http://localhost:5000`.

Default admin credentials: Username: `admin` / Password: `admin123` (Please change `ADMIN_INITIAL_PASSWORD` in `.env` or `docker-compose.yml` for production).

## 🛠️ Local Development (From Source)

### Prerequisites

- Rust 1.88+
- PostgreSQL 16+

### Steps

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Configure environment variables
cp .env.example .env
# Edit .env and ensure DATABASE_URL points to your PostgreSQL instance
# Example: DATABASE_URL=postgres://postgres:password@localhost:5432/llmeter

# 3. Build and run
cargo run

# Or build the release version
cargo build --release
./target/release/llmeter
```

The service will automatically run database migrations and create the default admin user upon startup.

## ⚙️ Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | Yes | - | PostgreSQL connection string |
| `AUTH_SECRET` | Yes | - | JWT signature secret. Use a strong random string in production |
| `ADMIN_INITIAL_PASSWORD` | No | `admin123` | Initial admin password |
| `PORT` | No | `5000` | Port the service listens on |

## 🔌 Proxy Usage Example

The proxy interface is compatible with original AI provider APIs. Just point your SDK's `base_url` to this service:

### Python (OpenAI SDK)

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:5000/v1",
    api_key="gc-xxxxxxxx"  # API Key created in the admin dashboard
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}]
)
```

### Node.js (OpenAI SDK)

```typescript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:5000/v1",
  apiKey: "gc-xxxxxxxx",
});

const response = await client.chat.completions.create({
  model: "gpt-4o",
  messages: [{ role: "user", content: "Hello!" }],
});
```

The same applies to Gemini and other providers. The system automatically matches routing rules based on the requested model name and forwards it to the correct provider.

## 📄 License

MIT License
