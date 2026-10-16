<p align="center">
  <img src="https://raw.githubusercontent.com/pichu2707/lazarobox-shell/main/docs/assets/lazarobox-shell-banner.png" alt="Lazarobox Shell" width="100%">
</p>

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

Desde crates.io (requiere Rust 1.88 o superior):

```bash
cargo install lazarobox-shell    # instala el binario `lazarobox-shell` en ~/.cargo/bin
```

Compatibilidad de plataformas:

- **Linux:** soportado. Es la única plataforma probada.
- **macOS:** el código compila (`cargo check --target aarch64-apple-darwin` pasa), pero no se ha probado en ejecución y no está soportado oficialmente. El directorio de trabajo de los paneles se obtiene en Linux mediante `/proc`; en macOS solo se actualiza si el shell emite OSC 7.
- **Windows:** no soportado.

Desde el código fuente:

```bash
cargo install --path .           # instala lazarobox-shell en ~/.cargo/bin
```

### Lanzador de escritorio

Para abrir lazarobox desde un menú de aplicaciones (rofi, GNOME, KDE…) sin tener una terminal abierta:

```bash
lazarobox-shell install-desktop                       # instala el lanzador "Lazarobox"
lazarobox-shell install-desktop --terminal alacritty  # fuerza una terminal concreta
lazarobox-shell install-desktop --uninstall           # lo quita
```

Funciona con cualquier instalación (`cargo install lazarobox-shell` o desde el código fuente) y con cualquier terminal. El comando escribe `<XDG_DATA_HOME>/applications/lazarobox.desktop` (por defecto `~/.local/share/applications`; `XDG_DATA_HOME` solo se respeta si es una ruta absoluta) y usa rutas absolutas, porque la sesión de escritorio suele no tener `~/.cargo/bin` en el `PATH`. Si el binario se mueve, vuelve a ejecutar el comando.

La terminal se elige en este orden:

1. `--terminal <cmd>`.
2. La variable `$TERMINAL`.
3. `xdg-terminal-exec`.
4. La primera encontrada en el `PATH`: `kitty`, `ghostty`, `wezterm`, `alacritty`, `foot`, `gnome-terminal`, `konsole`, `xfce4-terminal`, `x-terminal-emulator`.
5. Si no hay ninguna, el lanzador usa `Terminal=true` y el escritorio decide qué terminal abrir.

Para una terminal desconocida se usa `<cmd> -e <lazarobox-shell>` y el comando lo indica en su salida. Lazarobox está probado en Kitty: en otras terminales algunas teclas o el portapapeles por OSC 52 pueden no funcionar, y el comando lo avisa al instalar. Las rutas con caracteres especiales de la especificación Desktop Entry (espacios, comillas, `$`, `%`…) se rechazan en lugar de escaparse. `scripts/install-desktop.sh` sigue disponible como atajo que llama a este mismo comando.

### Anidamiento

Cada panel de shell define `LAZAROBOX=<pid>` (el pid de la instancia que lo abrió), igual que tmux hace con `$TMUX`, y los programas lanzados desde un panel la heredan. Si ejecutas `lazarobox-shell` dentro de un panel, no arranca: imprime `lazarobox-shell is already running in this terminal; use --force to nest anyway` y sale con código 1, antes de tocar la terminal. Para anidar a propósito usa `lazarobox-shell --force`; ten en cuenta que ambas instancias compartirán la misma tecla de prefijo.

---

## 📋 Modo COPY

Con `Ctrl+Espacio` y `[` se entra en modo COPY. Mueve el cursor con `h` `j` `k` `l`, selecciona con `v` (caracteres) o `V` (líneas completas) y copia con `y`: el texto llega al portapapeles del sistema mediante OSC 52 y se vuelve a TERMINAL. `Esc` limpia la selección (y una segunda pulsación sale). Las selecciones de más de 75 000 bytes no se copian.

Dentro de tmux, OSC 52 necesita `set -g set-clipboard on` en `tmux.conf`.

---

## 🖱️ Desplazamiento (scroll)

- **Rueda del ratón:** desplaza el panel que queda bajo el puntero, sin cambiar el foco. En un panel de shell recorre el historial (3 líneas por paso); en `less`, `man` o `vim` la rueda llega a la aplicación; en el chat de IA desplaza la conversación. Cualquier tecla pulsada en el panel (o un pegado) vuelve a la pantalla en vivo.
- **Selección nativa:** con la captura del ratón activa, mantén pulsado un modificador mientras arrastras para seleccionar texto con la terminal: `Shift` en Kitty y en la mayoría de terminales de Linux, `Option` en Terminal e iTerm2 de macOS.
- **Modo COPY:** `Ctrl+Espacio` y `[` funciona en paneles de shell y de chat. Se mueve con `j`/`k` (línea), `Ctrl+u`/`Ctrl+d` (media página), `gg` (inicio) y `G` (final); `Esc` o `q` sale. En un panel de shell, además, `v`, `V` e `y` seleccionan y copian.
- **Indicador `[+N]`:** un panel desplazado muestra en su esquina superior derecha cuántas líneas quedan por debajo; la línea de estado indica la posición (`↑N/total`) del panel enfocado.
- **Desactivar la captura:** `mouse = false` en `config.toml` (o la fila `Mouse` del menú `Ctrl+Espacio m`) deja la rueda y la selección a la terminal. Por defecto está activada.

---

## 🤖 Menú de IA

Con `Ctrl+Espacio` y `m` se abre el menú. Su sección **AI** muestra el motor general (**Set all tasks**) y una fila por tarea. `Enter` sobre una fila de tarea abre su selector de dos niveles (motor y modelo); `Esc` vuelve un nivel. La fila `custom…` permite escribir o pegar el nombre de un modelo. `r` sobre una fila de tarea o dentro del selector vuelve a detectar los motores.

`s` guarda la elección en la tabla `[ai]` de `config.toml` (y la de cada tarea en `[ai.tasks.*]`); `Esc` en el nivel superior la descarta. `Enter` nunca guarda ni cierra el menú. Mientras la elección difiera de la guardada, el pie del menú muestra `unsaved changes · s to save · Esc discards`.

El catálogo de modelos de Ollama es solo informativo y puede estar desactualizado. Los modelos del catálogo que no están instalados aparecen como `download required` y no se pueden elegir: **no se descarga nada**. Para usar uno, instálalo tú (`ollama pull <modelo>`) y pulsa `r`.

Se detectan estos motores: `ollama`, `lmstudio`, `llamacpp` y `vllm` (servidores locales) y los CLIs `claude`, `codex`, `gemini` y `opencode`. Los servidores se consultan en sus puertos habituales y los CLIs se buscan en el `PATH`.

La detección es de solo lectura y solo usa `127.0.0.1`: no se envían credenciales ni datos de tus paneles, y no se ejecuta nada salvo `--version` de los CLIs encontrados.

Límites de esta fase:

- No se admiten endpoints ni puertos personalizados.
- Los alias de shell no se detectan: el binario debe estar en el `PATH`.
- Al arrancar desde un lanzador de escritorio o rofi, el `PATH` suele ser mínimo. lazarobox pregunta una vez a tu shell de login (`$SHELL -l -i -c`, con un límite de unos 2,5 s; `/bin/sh` si `$SHELL` no está definido) y añade sus carpetas al final del `PATH` del proceso, tanto para detectar los CLIs como para lanzarlos en el chat. Si la consulta falla o tarda demasiado, se usa el `PATH` del proceso tal cual. El entorno del proceso no se modifica. La consulta ejecuta tus archivos rc interactivos al arrancar, así que cualquier demonio que lancen (por ejemplo `ssh-agent`, o un autoarranque de tmux o zellij) también se inicia en ese momento.

---

## 💬 Chat de IA (experimental)

Con `Ctrl+Espacio` y `c` se abre un panel de chat con un modelo, sin salir del workspace. Es una función **experimental**. Funciona con **Ollama** (local: debe estar instalado y en ejecución con `ollama serve`, y el modelo elegido debe estar descargado con `ollama pull <modelo>`) y con los CLIs **claude**, **codex** y **gemini**, que usan tu propio inicio de sesión (ver «Motores CLI: claude, codex y gemini»).

Teclas dentro del chat:

| Tecla                  | Acción                                              |
| :--------------------- | :-------------------------------------------------- |
| `Enter`                | Envía el mensaje                                    |
| `Alt+Enter` / `Ctrl+J` | Salto de línea en el mensaje                        |
| `Esc` / `Ctrl+C`       | Cancela la respuesta en curso                       |
| `Ctrl+R`               | Muestra u oculta el razonamiento del modelo         |
| `Ctrl+Y` / `Ctrl+B`    | Copia la última respuesta / su último bloque de código (OSC 52) |
| `PageUp` / `PageDown`  | Desplaza la conversación                            |

**Perfiles y prompts:** `profile = "analyst"` o `"programmer"` (por defecto) en `[ai]`, o la fila Profile del menú de IA, elige el _system prompt_ integrado. Puedes sustituirlo con `~/.config/lazarobox/prompts/<perfil>/<tarea>.md` (después `general.md`; máx. 16 KiB). El prompt incluye un bloque de contexto del workspace (proyecto, rama, directorio relativo, SO, shell y fecha; nunca rutas absolutas ni contenido); se desactiva con `context = false` o la fila Context del menú.

**Aviso al terminar:** con `notify = true` en `[ai]` (o la fila Notify del menú de IA; desactivado por defecto), el chat emite la campana del terminal cuando una respuesta termina y no la estás mirando (otro panel, pestaña o ventana).

Para cerrar el panel: `Ctrl+Espacio`, `w`, `q`. La respuesta se muestra mientras se genera; si el modelo "piensa" (por ejemplo `qwen3`), la barra de estado muestra un indicador `thinking` y el razonamiento queda en un bloque aparte.

**Motor y modelo:** el chat usa la tarea `chat`. Se elige en el menú de IA (`Ctrl+Espacio` y `m`) o a mano en `config.toml`:

```toml
[ai.tasks.chat]
engine = "ollama"
model = "llama3.2"   # o cualquier modelo que tengas descargado
```

Si la tarea `chat` no tiene motor propio, usa el motor general (`[ai]`). El panel indica qué falta cuando no hay motor, no está en marcha o no está soportado.

**Privacidad:**

- Con **Ollama** todo es local: el chat solo habla con `127.0.0.1` y no envía credenciales (la memoria opcional de engram tiene su propia sección más abajo).
- Con **claude, codex y gemini** la conversación (tus mensajes, el prompt del perfil y, si la activas, la memoria del proyecto) **se envía al proveedor** del CLI, con tu cuenta y bajo sus condiciones. lazarobox no lee ni copia tus credenciales.
- Las conversaciones se guardan en un `chat.db` **sin cifrar**, texto plano con permisos `0600`, en `$XDG_DATA_HOME/lazarobox/chat.db` (por defecto `~/.local/share/lazarobox/chat.db`). Para borrarlas todas, cierra lazarobox y elimina ese archivo (y los auxiliares `chat.db-journal` si existen). Una conversación se borra por separado desde la lista (`d`, luego `y`).
- El razonamiento del modelo se muestra en el panel pero **nunca** se guarda en `chat.db` (ver «Historial de conversaciones»).

Límites de esta versión:

- Los motores `lmstudio`, `llamacpp`, `vllm` y `opencode` se detectan, pero aún no tienen adaptador de chat.
- Las etiquetas extra de una conversación vuelven en orden alfabético al reabrirla.

### Motores CLI: claude, codex y gemini

El chat puede hablar con los CLIs **claude**, **codex** y **gemini** que ya tengas instalados y con la sesión iniciada (`claude`, `codex login`, `gemini`). lazarobox no pide ni guarda claves de API: arranca el CLI, le pasa la conversación por la entrada estándar y lee su respuesta. Se eligen como cualquier otro motor, en el menú de IA o en `[ai.tasks.chat]`.

Versiones comprobadas: claude 2.1.299, codex-cli 0.142.5 y gemini 0.63.0. Otras versiones pueden fallar (ver «Si un CLI cambia»).

**Solo texto.** El chat es de texto: el CLI se lanza con opciones fijas que le quitan las herramientas, de modo que no ejecuta comandos, no edita archivos ni usa servidores MCP. Pedirle «crea un archivo» no crea nada. Es la primera capa; la segunda es la jaula.

**La jaula (Landlock, solo Linux).** Cada turno de un CLI se ejecuta dentro de una jaula del núcleo (Landlock, ABI 3 o superior; lazarobox se relanza a sí mismo para aplicarla antes de arrancar el CLI). La jaula **bloquea**:

- la lectura de todo lo que no se haya concedido: tu `$HOME`, `~/.ssh`, `~/.gnupg`, las carpetas de otros CLIs (`codex` no puede leer `~/.claude`) y `/tmp`;
- la escritura fuera de la carpeta de estado del propio CLI (`~/.claude` y `~/.claude.json`, `$CODEX_HOME` o `~/.codex`, `~/.gemini`) y de un directorio temporal privado de cada turno;
- la ejecución de programas: solo se puede ejecutar el intérprete (node) y el propio CLI. `ssh`, `curl`, `bash` o `git` no se pueden lanzar directamente;
- abrir puertos TCP para escuchar (desde Landlock ABI 4).

El directorio de trabajo del CLI es una carpeta vacía y privada, el entorno se limpia (`SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, agentes y variables de otras sesiones) y no se usa ninguna shell.

**Disponibilidad por sistema:**

- **codex y gemini** exigen la jaula. Sin ella (otro sistema operativo, un núcleo sin Landlock o una instalación que la jaula rechaza) no se lanza ningún proceso y el panel indica `<motor>: not available on this system yet`.
- **claude** funciona también sin la jaula: en ese caso el panel avisa una vez con `claude runs without the jail here: <motivo>`. En ese modo solo lo protegen las opciones de solo texto.
- Fuera de Linux no hay jaula por ahora.

**Riesgos que quedan (léelos):**

- **La jaula limita, no es un sello.** Landlock no controla `mmap` con permiso de ejecución, así que el cargador dinámico que necesita un CLI de scripts podría arrancar un programa legible; por eso se retiran de la lectura `/usr/bin`, `/usr/sbin` y `/bin`. Los programas que quedan bajo `/usr/lib` siguen siendo alcanzables. Las restricciones de `execve` son defensa en profundidad. La red **no** está restringida: un CLI puede conectar a cualquier destino (es lo que necesita para hablar con su proveedor).
- **Las carpetas de estado son escribibles.** El CLI debe poder renovar su sesión, así que `~/.claude`, `$CODEX_HOME`/`~/.codex` y `~/.gemini` están en modo escritura. Si un modelo consiguiera que el CLI escribiera ahí una configuración (por ejemplo, hooks o servidores MCP), una ejecución posterior de ese CLI **sin** la jaula (tú, en tu terminal) la ejecutaría con tus permisos.
- **claude reescribe `~/.claude.json` en el sitio.** Dentro de la jaula no puede crear el archivo temporal junto a `~/.claude.json`, y escribe directamente sobre él. Un corte en mitad de la escritura, o dos claude a la vez, pueden dejarlo dañado. Se está estudiando `CLAUDE_CONFIG_DIR` para evitarlo.
- **El _system prompt_ de claude se ve en `ps`.** claude recibe el prompt del perfil como argumento de línea de comandos, visible para cualquier proceso local mientras dura el turno. codex y gemini lo reciben por la entrada estándar y en `ps` solo se ven opciones fijas y el nombre del modelo.
- **La respuesta es la palabra del modelo.** Los modelos pueden negarse o inventar una justificación («no tengo permisos»). La garantía real está en la ausencia de herramientas y en la jaula, no en lo que diga el modelo.
- **Comprobación pendiente:** la línea base de llamadas al sistema con `strace` (M12/M14) no se ha tomado porque `strace` no estaba instalado. En su lugar se midió qué programas arranca cada turno (solo node y el CLI nativo) y que el turno funciona con la jaula completa.

**claude.** Usa `--tools ""`, sin servidores MCP, sin comandos con barra y sin guardar sesión (`--no-session-persistence`). El prompt del perfil sustituye al prompt integrado del CLI.

**codex.** La lista de modelos sale de **tu cuenta**: al detectar los motores, lazarobox ejecuta una vez `codex debug models` (con un límite de 4 s, sin entrada estándar, con el entorno depurado y en la carpeta privada de lazarobox), dentro de la misma jaula que usa el chat de codex, de modo que solo puede escribir en la carpeta de estado de codex (`$CODEX_HOME` o `~/.codex`). Igual que al usar codex, ese comando puede actualizar el catálogo desde OpenAI con tu sesión y renovar la caché de codex (`models_cache.json`). Sin la jaula no se ejecuta y solo aparece `default`. El resultado se guarda en memoria y se renueva como máximo cada 10 minutos (al arrancar y al abrir el menú se reutiliza); la tecla `r` del menú lo vuelve a pedir siempre. Si una renovación falla, se conserva la lista anterior mientras tenga menos de 10 minutos. El selector muestra `default (CLI picks)` y, a continuación, los modelos que tu cuenta lista, con su nombre visible; lo que se guarda en `config.toml` es siempre el identificador (por ejemplo `gpt-6.1-sol`). Con un codex antiguo, sin ese subcomando, o si el comando falla, solo aparece `default`. Un modelo asignado o escrito en `custom…` que no esté en la lista de tu cuenta se marca `not in your account's list`; es solo un aviso y no bloquea la elección, porque la lista puede estar desactualizada. Elige un modelo que **tu cuenta admita**. Con una cuenta de ChatGPT, solo algunos modelos funcionan: el modelo por defecto de codex (por ejemplo `gpt-5.5`) puede fallar con `The '<modelo>' model is not supported when using Codex with a ChatGPT account`. Pon el nombre en `[ai.tasks.chat] model = "..."` o en la fila `custom…` del selector. codex usa `$CODEX_HOME` si está definido. Se lanza sin guardar sesión (`--ephemeral`), con el sandbox de solo lectura y con todas sus herramientas desactivadas.

**gemini.** Notas:

- Elige un modelo que tu cuenta tenga disponible; el modelo automático puede responder «high demand» (503), y algunos modelos agotan la cuota diaria o ya no se ofrecen a cuentas nuevas. Un error del proveedor aparece en el panel con su texto.
- gemini **guarda sus propias sesiones** en `~/.gemini/tmp/cli-cwd/chats/` (archivos `session-*.jsonl`) y no hay opción para evitarlo. Esas copias quedan **fuera de `chat.db`**: no se borran al borrar una conversación ni por retención, y no tienen los permisos `0600` de `chat.db`. Elimínalas tú si lo necesitas.
- gemini lee tu configuración propia de `~/.gemini` (`GEMINI.md`, `skills/`, `settings.json`), que va como contexto al proveedor.
- Con gemini, el modo `plan` **no** basta como barrera (el modelo puede salir de él). El chat desactiva las herramientas con una política que deniega todas (un archivo temporal que lazarobox escribe en la carpeta privada del turno), sin extensiones y sin servidores MCP.
- Si tu `node` es el de Homebrew (sus bibliotecas viven bajo `.linuxbrew/Cellar` y `opt`), la jaula concede en solo lectura exactamente las carpetas de bibliotecas que el propio ejecutable declara y los archivos `openssl.cnf` y `cert.pem` de Homebrew; nunca el prefijo completo. Esta regla vale para cualquier CLI cuyo ejecutable (el intérprete de un script o el binario nativo) declare un RUNPATH o RPATH en su ELF, no solo para `node`.

**Si un CLI cambia.** Las listas de herramientas desactivadas son fijas y los decodificadores aceptan solo los tipos de evento que conocen:

- codex **falla cerrado**: si una versión nueva añade un tipo de elemento o retira una opción que lazarobox desactiva (`Unknown feature flag`), el chat muestra un error hasta que se actualice lazarobox. gemini hace lo mismo con los tipos de evento desconocidos.
- Un `node` ubicado en una carpeta con permiso de escritura para el grupo (por ejemplo con `umask 002`), o bajo `/tmp`, se **rechaza** para los CLIs de scripts dentro de la jaula: se prueba primero el `node` instalado junto al CLI y luego los del `PATH`, y si ninguno es de confianza el CLI no arranca (si es obligatorio, el panel dice que no está disponible).

### Historial de conversaciones

Los mensajes se guardan en `chat.db` a medida que conversas. Una respuesta que se interrumpe tras haber recibido texto (por un error o una cancelación) también se guarda, marcada como parcial. `Ctrl+Espacio` y `c` abre la **lista de conversaciones** del proyecto actual (el repositorio git), de la más reciente a la más antigua, con el formato `fecha · perfil · primer mensaje`.

| Tecla        | Acción                                                                          |
| :----------- | :------------------------------------------------------------------------------ |
| `Enter`      | Retoma la conversación: carga el historial completo y sigue en la misma, con su perfil guardado |
| `n` / `Esc`  | Empieza una conversación nueva                                                  |
| `a`          | Alterna entre las conversaciones del proyecto y las de todos los proyectos      |
| `d`, luego `y` | Borra la conversación (cualquier otra tecla cancela); si está abierta en otro panel, ese panel deja de guardar. Con memoria activada, `ENGRAM_HTTP_TOKEN` definido y notas guardadas, la pregunta ofrece también `a`: borra la conversación y después sus notas en engram (ver «Memoria») |
| `t`          | Edita las etiquetas de la fila                                                  |
| `Ctrl+Espacio` `T` | Edita las etiquetas de la conversación abierta                            |

**Etiquetas:** cada conversación lleva la etiqueta del proyecto (primero) y hasta 8 etiquetas extra. El editor es una sola línea separada por comas: `Enter` guarda y `Esc` descarta. Guardar cuenta como actividad, así que la fila sube al principio de la lista.

Límites: al reabrir una conversación, las etiquetas extra salen en orden alfabético. La combinación `Ctrl+Espacio` `T` aún no se ha comprobado a mano en Kitty.

### Memoria del proyecto (engram)

El chat puede actuar como cliente de [engram](https://github.com/Gentleman-Programming/engram) y compartir la memoria del proyecto con Claude Code, opencode y otros agentes: al abrir el chat lee el contexto del proyecto, puedes guardar **notas** durante la conversación y generar un **resumen** de la sesión. Es opcional y está **desactivada** por defecto.

Se activa desde el menú de IA (`Ctrl+Espacio` y `m`), que antes muestra una advertencia de privacidad que debes aceptar una sola vez, o a mano en `config.toml`:

```toml
[ai.memory]
enabled = true        # por defecto false
acknowledged = true   # advertencia de privacidad aceptada
port = 7437           # por defecto ENGRAM_PORT si es válido, si no 7437
```

**Privacidad (léelo antes de activarla):**

- Las notas salen del `chat.db` privado (permisos `0600`) y pasan a ser legibles por **cualquier proceso local y cualquier agente que use engram**, y por `engram sync` si lo tienes activado.
- Las rutas HTTP de engram en `127.0.0.1` están abiertas: lazarobox no puede restringir quién las lee.
- Antes de enviar una nota se ocultan los secretos reconocibles. El razonamiento del modelo no se guarda nunca.

**Requisitos:**

- Solo proyectos **git**; en otro directorio la memoria indica `memory needs a git project`.
- Un servidor `engram` 3.x en marcha. Si no responde, lazarobox ofrece lanzarlo: muestra la ruta **absoluta** del binario, `y` lo inicia y `n` o `Esc` lo deja para otra vez. La oferta solo aparece antes del primer mensaje de un chat inactivo. El servidor lanzado **sigue en marcha cuando cierras lazarobox** (lazarobox nunca lo detiene) y su salida va a `$XDG_STATE_HOME/lazarobox/engram-serve.log` (o `~/.local/state/lazarobox/engram-serve.log`), un archivo privado `0600`.
- `ENGRAM_PORT` fija el puerto por defecto. `ENGRAM_HTTP_TOKEN` es el token del servidor engram: lazarobox solo lo usa para **borrar** notas, y solo cuando tú lo pides con `a` al borrar una conversación (ver «Borrar notas»). Solo comprueba si está definido y no vacío; nunca lo guarda ni lo muestra.

**Teclas** (dentro del chat):

| Tecla                | Acción                                                     |
| :------------------- | :--------------------------------------------------------- |
| `Ctrl+Espacio` `n`   | Nueva nota (`Ctrl+S` guarda, `Esc` cierra)                 |
| `Ctrl+Espacio` `o`   | Muestra u oculta el bloque de memoria                      |
| `Ctrl+Espacio` `s`   | Resumen de la sesión (`Ctrl+S` aprueba, `Esc` lo descarta) |

La barra de estado muestra `mem: ready`, `mem: checking` o `mem: unavailable`.

**Encontrar las notas en engram:** el _topic key_ de cada nota empieza por `chat/` (`chat/<tipo>/<título>`), por ejemplo `chat/decision/use-axum`.

**Volver a una versión anterior:** la memoria sube el esquema de `chat.db` a la versión 2. Un lazarobox más antiguo se niega a abrir un `chat.db` más nuevo que él y lo deja intacto; al volver a actualizar, vuelve a estar disponible.

**Borrar notas:** una nota y una conversación tienen vidas separadas, así que borrar la conversación (`d`, `y`) deja sus notas en engram. Si la memoria está activada, `ENGRAM_HTTP_TOKEN` está definido y la conversación tiene notas, la pregunta pasa a `Delete conversation? y · n · a also delete its N notes`: `y` borra solo la conversación, `a` borra la conversación y después sus notas (`DELETE /observations/{id}` con el token, una por una), y cualquier otra tecla cancela. Sin token o con la memoria desactivada la pregunta es la normal y, tras borrar, una línea indica que las notas siguen en engram. El resultado de `a` se muestra como `deleted N notes from engram`, `deleted K of N notes from engram` o `could not delete notes: <motivo>`; la conversación ya está borrada en cualquiera de los casos. Al superar el límite de 200 conversaciones, la más antigua se elimina **sin** tocar sus notas.

**Retomar una conversación:** al retomarla se reutiliza la sesión de engram que usaron sus notas (con su proyecto), así que las notas nuevas siguen en la misma sesión y se cierra una sola vez. Una conversación retomada que no escribe nada no cierra ninguna sesión. Sus notas nuevas siguen en el proyecto de engram con el que se guardó la conversación, aunque la retomes desde otro directorio.

Límites: la memoria se envía con la conversación a los adaptadores de Ollama, claude, codex y gemini (en los tres CLIs, **al proveedor**).

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
