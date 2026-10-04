# 🚀 LAZAROBOX SHELL (`lazarobox-shell`)

> AI-powered, domain-agnostic TUI workspace for Developers, SEO/CRO Analysts, and Industrial Automation Engineers. Built in **Rust**.

---

## 📸 Overview & Vision

**`lazarobox-shell`** es un espacio de trabajo unificado (_Workspace_) de terminal (TUI) ultrarrápido, modular e independiente escrita en **Rust**. Actúa como una interfaz centralizada para orquestación de modelos de Inteligencia Artificial (locales y en la nube), monitorización de métricas en tiempo real y ejecución de herramientas de automatización.

Aunque nace optimizada para flujos de trabajo en **Desarrollo de Software, SEO, CRO y Analítica Digital**, su núcleo está diseñado para ser **completamente agnóstico al dominio**, permitiendo escalar hacia sectores como **Automatización Industrial, IoT, Logística y Telemetría SCADA/PLC**.

---

## ✨ Principios de Diseño

- **⚡ Rendimiento e Integridad Cero-Latencia:** Desarrollada 100% en Rust utilizando `tokio` para asincronía y `ratatui` + `crossterm` para un renderizado TUI rápido y ligero.
- **🎯 Arquitectura Centrada en Proyectos (360°):** La información se organiza por unidades de negocio o proyectos en una vista consolidada que integra métricas de desarrollo, analítica, SEO, conversión y automatización.
- **🤖 IA Multimodelo Agnóstica (`llm-router`):** Conmutación en caliente entre modelos locales (`Ollama`, `llama.cpp`, `vLLM`) y APIs en la nube (`Anthropic Claude`, `Google Gemini`, `OpenAI`).
- **📊 Visualización Rica & Soporte Multimedia:** Gráficos vectoriales TUI (Line/Bar charts) y soporte nativo de renderizado de imágenes reales directamente en la terminal mediante los protocolos **Kitty Graphics Protocol** y **Sixel**.
- **🔐 Autenticación y Seguridad Cero Fricción:** Almacenamiento seguro de tokens en el Keyring nativo del sistema operativo (GNOME Keyring, macOS Keychain, Windows Credential Manager) o cifrado AES-256 local, con flujos OAuth2 efímeros para Google y Microsoft.
- **🧩 Extensibilidad vía MCP (Model Context Protocol):** Conexión de herramientas y scripts externos en Rust/Python mediante el protocolo estándar MCP.

---

## 🎨 Identidad Visual (`lazarobox-theme`)

Inspirado en la paleta de colores de lazarobox (tonos oscuros suavizados con acentos cian/turquesa):

| Elemento                       | Código HEX            | Descripción                                                   |
| :----------------------------- | :-------------------- | :------------------------------------------------------------ |
| **Fondo Principal**            | `#181E24`             | Azul/Gris oscuro profundo para reducir fatiga visual          |
| **Contenedores y Paneles**     | `#202831`             | Gris fosc de contraste y elevación                            |
| **Acento Primario / Prompt**   | `#00E5FF` / `#89DCEB` | Cian / Turquesa para comandos, cursor y marca                 |
| **Acento IA / MCP**            | `#CBA6F7`             | Violeta / Púrpura para llamadas a modelos y herramientas      |
| **Métricas: Éxito / Subida**   | `#A6E3A1`             | Verde salvia suave para conversiones y mejoras SEO            |
| **Métricas: Alerta / Warning** | `#FAB387`             | Naranja cálido para alertas de rendimiento o mantenimiento    |
| **Métricas: Error / Caída**    | `#F38BA8`             | Coral / Rojo sobrio para caídas SERP o errores de compilación |

---

## 📁 Arquitectura del Workspace en Rust

```text
lazarobox-shell/
├── Cargo.toml
├── README.md
├── docs/
│   └── ARCHITECTURE.md
├── config/
│   └── default_theme.toml
└── src/
    ├── main.rs                   # Entrypoint & CLI arguments parser
    ├── app.rs                    # Central Application State & Event Loop
    │
    ├── ui/                       # CAPA DE INTERFAZ TUI (Ratatui)
    │   ├── mod.rs
    │   ├── layout.rs             # Modulos de pantallas, splits y modales
    │   ├── components/
    │   │   ├── tabs.rs           # Barra superior de pestañas/proyectos
    │   │   ├── statusline.rs     # Barra de estado inferior
    │   │   ├── chat_panel.rs     # Interfaz de chat streaming con la IA
    │   │   └── widgets/
    │   │       ├── metrics.rs    # KPI cards & Sparklines
    │   │       ├── charts.rs     # BarCharts & Line Charts (TUI Vectorial)
    │   │       └── image_view.rs # Renderizador Kitty / Sixel
    │   └── theme.rs              # Definición de la paleta lazarobox
    │
    ├── core/                     # NÚCLEO Y LÓGICA DE NEGOCIO
    │   ├── mod.rs
    │   ├── project.rs            # Gestor de contexto de proyecto (project.toml)
    │   └── config.rs             # Lector/Escritor de la configuración global
    │
    ├── llm/                      # ENRUTADOR Y PROVEEDORES DE IA
    │   ├── mod.rs
    │   ├── router.rs             # Trait LlmProvider y enrutado dinámico
    │   ├── providers/
    │   │   ├── ollama.rs         # Cliente API local Ollama
    │   │   ├── anthropic.rs      # Cliente API Claude
    │   │   ├── gemini.rs         # Cliente API Google Gemini
    │   │   └── openai.rs         # Cliente compatible con OpenAI
    │   └── mcp/                  # CLIENTE MODEL CONTEXT PROTOCOL
    │       ├── mod.rs
    │       └── client.rs         # Descubrimiento y ejecución de herramientas MCP
    │
    └── auth/                     # SISTEMA DE SEGURIDAD Y AUTENTICACIÓN
        ├── mod.rs
        ├── keyring.rs            # Integración con Keyring nativo del SO / AES Vault
        └── oauth.rs              # Servidor efímero PKCE para Google/Microsoft
```
