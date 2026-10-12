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

## 📦 Instalación

```bash
cargo install --path .           # instala lazarobox-shell en ~/.cargo/bin
scripts/install-desktop.sh       # añade el lanzador "Lazarobox" (rofi, menú de aplicaciones)
```

El lanzador abre `lazarobox-shell` dentro de Kitty (`--class lazarobox`), así que no hace falta tener una terminal abierta. Requiere tener `kitty` y `lazarobox-shell` instalados; si falta alguno, el script termina con error. Las rutas de ambos se resuelven al instalar; si cambian, vuelve a ejecutar el script. Respeta `XDG_DATA_HOME` y `CARGO_HOME`. Para quitarlo: `scripts/install-desktop.sh --uninstall`.

---

## 📋 Modo COPY

Con `Ctrl+Espacio` y `[` se entra en modo COPY. Mueve el cursor con `h` `j` `k` `l`, selecciona con `v` (caracteres) o `V` (líneas completas) y copia con `y`: el texto llega al portapapeles del sistema mediante OSC 52 y se vuelve a TERMINAL. `Esc` limpia la selección (y una segunda pulsación sale). Las selecciones de más de 75 000 bytes no se copian.

Dentro de tmux, OSC 52 necesita `set -g set-clipboard on` en `tmux.conf`.

---

## 🤖 Menú de IA

Con `Ctrl+Espacio` y `m` se abre el menú. Su sección **AI** muestra el motor general (**Set all tasks**) y una fila por tarea. `Enter` sobre una fila de tarea abre su selector de dos niveles (motor y modelo); `Esc` vuelve un nivel. La fila `custom…` permite escribir o pegar el nombre de un modelo. `r` sobre una fila de tarea o dentro del selector vuelve a detectar los motores.

`s` guarda la elección en la tabla `[ai]` de `config.toml` (y la de cada tarea en `[ai.tasks.*]`); `Esc` en el nivel superior la descarta. `Enter` nunca guarda ni cierra el menú.

El catálogo de modelos de Ollama es solo informativo y puede estar desactualizado. Los modelos del catálogo que no están instalados aparecen como `download required` y no se pueden elegir: **no se descarga nada**. Para usar uno, instálalo tú (`ollama pull <modelo>`) y pulsa `r`.

Se detectan estos motores: `ollama`, `lmstudio`, `llamacpp` y `vllm` (servidores locales) y los CLIs `claude`, `codex`, `gemini` y `opencode`. Los servidores se consultan en sus puertos habituales y los CLIs se buscan en el `PATH`.

La detección es de solo lectura y solo usa `127.0.0.1`: no se envían credenciales ni datos de tus paneles, y no se ejecuta nada salvo `--version` de los CLIs encontrados.

Límites de esta fase:

- No se admiten endpoints ni puertos personalizados.
- Los alias de shell no se detectan: el binario debe estar en el `PATH`.

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
