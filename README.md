<p align="center">
  <img src="https://raw.githubusercontent.com/pichu2707/lazarobox-shell/main/docs/assets/lazarobox-shell-banner.png" alt="Lazarobox Shell" width="100%">
</p>

# 🚀 LAZAROBOX SHELL (`lazarobox-shell`)

[![CI](https://github.com/pichu2707/lazarobox-shell/actions/workflows/ci.yml/badge.svg)](https://github.com/pichu2707/lazarobox-shell/actions/workflows/ci.yml)

> AI-powered, domain-agnostic TUI workspace for Developers, SEO/CRO Analysts, and Industrial Automation Engineers. Built in **Rust**.

---

## 📸 Overview & Vision

**`lazarobox-shell`** es un espacio de trabajo unificado (_Workspace_) de terminal (TUI) ultrarrápido, modular e independiente escrita en **Rust**. Actúa como una interfaz centralizada para conversación con modelos de Inteligencia Artificial (locales y a través de CLIs), panes de terminal y ejecución de herramientas de automatización bajo aprobación del usuario.

Aunque nace optimizada para flujos de trabajo en **Desarrollo de Software, SEO, CRO y Analítica Digital**, su núcleo está diseñado para ser **completamente agnóstico al dominio**, permitiendo escalar hacia sectores como **Automatización Industrial, IoT, Logística y Telemetría SCADA/PLC**.

---

## ✨ Principios de Diseño

- **⚡ Rendimiento e Integridad Cero-Latencia:** Desarrollada 100% en Rust utilizando `tokio` para asincronía y `ratatui` + `crossterm` para un renderizado TUI rápido y ligero.
- **🎯 Arquitectura Centrada en Proyectos:** Cada pestaña agrupa los panes de terminal y el chat de un proyecto. Planificado: dashboards básicos en el shell y lectura de datos, con la analítica como objetivo.
- **🤖 IA Multimotor:** Chat con los CLIs `claude`, `codex` y `gemini` y, por HTTP local, con `Ollama`, `LM Studio`, `llama.cpp` y `vLLM`. La API de OpenAI en la nube y `opencode` aún no tienen adaptador de chat.
- **🧩 Extensibilidad vía MCP (Model Context Protocol):** lazarobox usa los servidores MCP que configures y expone sus propias herramientas a los CLIs.

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

Desde crates.io. Necesitas Rust (con `cargo`) 1.88 o superior; si no lo tienes, instálalo antes desde <https://rustup.rs>. Después, en una terminal:

```bash
cargo install lazarobox-shell --locked    # instala el binario `lazarobox-shell` en ~/.cargo/bin
```

`--locked` compila con las mismas versiones de dependencias que se probaron al publicar (las del `Cargo.lock` del paquete). Sin él, cargo vuelve a resolver cada dependencia a su última versión compatible, lo que casi siempre funciona igual, pero una dependencia recién publicada con un fallo puede romper la compilación.

**Actualizar.** Dentro de lazarobox: abre el menú de configuración, ve a **Updates** y pulsa `Enter`. Se abre una pestaña que descarga y compila la versión nueva (puede tardar unos minutos; deja lazarobox abierto). Cuando diga `Updated to X.Y.Z`, reinicia lazarobox para usarla. Si lazarobox no puede actualizarse solo, lo dice y `Enter` o `y` copia al portapapeles el mismo comando de instalación: pégalo en una terminal.

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

### Al cerrar lazarobox

Al arrancar, lazarobox lanza un vigilante (`lazarobox-shell __watch`, un proceso pequeño en su propia sesión; en Linux `ps` lo muestra como `/proc/self/exe __watch`) que le sobrevive. Cada grupo de procesos que lazarobox arranca (los turnos de claude, codex y gemini, tus servidores MCP, el `llama-server` gestionado y las comprobaciones cortas, como `--version`) se apunta en él, y se borra cuando lazarobox lo recoge. Cuando lazarobox termina, sea como sea (al salir, si se cae, con `kill -9` o al cerrar la pestaña de la terminal; en Linux y en macOS), el vigilante manda `SIGHUP` y `SIGTERM` a los grupos que quedan apuntados, les da 3 s, manda `SIGKILL` a lo que siga vivo y sale.

- **Al salir, lazarobox hace lo mismo él mismo, con suavidad:** en cuanto te devuelve la pantalla manda `SIGHUP` y `SIGTERM` a todo lo que lanzó y solo manda `SIGKILL` a lo que siga en marcha 2 s después, mientras guarda el historial y la memoria. Un programa que termina al recibir la señal no espera nada más; lo que dejó detrás (sus hijos) recibe entonces `SIGKILL`. Un turno que sigue escribiendo su respuesta no se corta antes de esos 2 s. Si cierras la pestaña o matan a lazarobox a mitad de la salida, el vigilante repite la misma secuencia, y ahí también los hijos que dejó un programa tienen esos segundos para terminar.
- Solo toca un grupo si su proceso líder sigue siendo el que se apuntó (misma hora de arranque) o ya no tiene líder: un pid reutilizado por otro programa nunca recibe la señal.
- `engram serve` no se apunta nunca: es tu servidor de memoria, lo comparten otras herramientas y sigue en marcha a propósito.
- Los shells de los paneles no pasan por el vigilante: al cerrarse la terminal de lazarobox reciben `SIGHUP`, como en cualquier terminal. Lo que lanzaste en un panel (`npm run dev &`, `nohup`, `disown`) sigue su curso como en cualquier terminal, pero lazarobox te avisa:
  - la pregunta de salida lo nombra: `Quit? still running: sleep 77, npm run dev (y/n)` (como mucho tres, y `+N more` para el resto; si la barra es estrecha, solo cuántos). También nombra `llama-server (shared)` si el llama-server gestionado que arrancó está respondiendo a otro lazarobox (ver «llama.cpp gestionado»);
  - al salir, con la pregunta o con `exit` en el último panel, lo escribe en tu terminal en cuanto te la devuelve, con su grupo de procesos para que puedas pararlo (`kill -- -<grupo>`).
  - Cuenta también lo de un panel que ya cerraste. Un programa que salió de la sesión del panel (`setsid`) no se nombra.
- Si el vigilante no puede arrancar, lazarobox funciona igual que antes: al salir para lo que lanzó, pero una caída puede dejarlo en marcha.

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
- **Desactivar la captura:** `mouse = false` en `config.toml` (o la fila `Mouse` de la sección Settings del menú `Ctrl+Espacio m`) deja la rueda y la selección a la terminal. Por defecto está activada.

---

## ⚙️ Menú: sección Settings

`Ctrl+Espacio` y `m` abre el menú. Con la sección de IA, el menú se divide en pestañas, **Settings** y **AI**, que se cambian con `Tab` y `Shift+Tab`; `j`/`k` se mueven y dan la vuelta dentro de la pestaña activa, y cada pestaña recuerda su fila. Al abrir el menú siempre empieza en **Settings**. El aviso de actualización solo se muestra en esa pestaña, y su etiqueta lleva un `*` mientras haya una actualización pendiente. Dentro de un selector, del campo `custom…` o de la advertencia de privacidad, `Tab` no hace nada. Su sección **Settings** tiene, en este orden:

| Fila | Qué hace | Clave en `config.toml` |
| ---- | -------- | ---------------------- |
| `Statusline position` | Barra de estado arriba o abajo (`h`/`l`) | `[statusline] position` |
| `Tab bar position` | Barra de pestañas arriba o abajo | `[tabbar] position` |
| `Mouse` | Captura del ratón | `mouse` |
| `Notify` | Aviso al terminar una respuesta del chat (`off`, `auto`, `desktop`, `bell`); `t` lo prueba | `[ai] notify` |
| `Sound` | Sonido propio del aviso; `t` lo prueba | `[ai] notify_sound` |
| `llama-server idle` | Minutos sin uso tras los que se para el llama-server gestionado: `5 min`, `15 min`, `30 min` o `never` | `[ai.servers.llamacpp] idle_minutes` |
| `Updates` | Versión en ejecución y actualización disponible (ver abajo) | `[updates] check` (solo en el fichero) |
| `Feedback` | Abre el formulario para informar de un fallo o proponer una idea (ver abajo) | — |

`Notify` y `Sound` antes estaban en la sección **AI**; solo cambian de sitio, y sus claves y su comportamiento siguen igual.

### Feedback: fallos e ideas desde el terminal

`Enter` sobre **Feedback** abre un formulario corto:

- **Kind**: `Bug report` o `Idea or goal` (`←`/`→`).
- **Title** y **Description** (`Tab` pasa al siguiente campo; en la descripción, `Enter` añade una línea).
- **Attach the environment** (`Espacio` lo activa o desactiva): la versión de lazarobox, el sistema (y si es WSL), la shell, la terminal (`TERM_PROGRAM`) y los motores de IA detectados con su versión. Se ve entero antes de enviarlo; nunca incluye rutas, nombres de usuario ni claves.

`Ctrl+S` envía el issue a GitHub con tu propio `gh` (`gh issue create`): el de fallos con la etiqueta `bug` o el de ideas con `idea` (las ideas llegan al tablero de Goals del proyecto). El issue sigue la forma de los formularios del repositorio: una sección `### What happened` (o `### The idea`) con la descripción y, si lo adjuntas, `### Environment`. Mientras se envía, el pie dice `Sending to GitHub…`; al terminar, el formulario muestra `Sent:` y el enlace del issue, que se abre con `Ctrl+O` o con un clic. Si `gh` falla, el formulario se queda con todo lo escrito y el pie muestra su mensaje; `Ctrl+S` lo vuelve a intentar. **lazarobox solo envía algo cuando pulsas `Ctrl+S`, y siempre a través de tu sesión de `gh`: no guarda ningún token de GitHub.** GitHub solo conserva las etiquetas si tienes permiso de escritura en el repositorio.

Si `gh` no está instalado o no has iniciado sesión (`gh auth login`), `Ctrl+S` abre en el navegador la página del issue ya rellena y el formulario explica por qué. `Ctrl+O` abre esa misma página directamente, para revisarla y enviarla desde allí (con `xdg-open`; en WSL con `wslview` o `rundll32.exe url.dll,FileProtocolHandler`; en macOS con `open`; si no hay ninguno, copia el enlace al portapapeles con OSC 52).

Antes de enviar nada, el título y la descripción pasan por el mismo filtro de secretos que las notas de memoria; si enmascara algo, el formulario lo muestra (`Secrets were masked; review and press Ctrl+S again`) para que lo revises. Un enlace de más de 8000 caracteres no se puede abrir (`Too long for a link: shorten the description`), pero sí enviar con `Ctrl+S`.

Desde la web, los formularios de issue de GitHub piden los mismos datos.

---

## 🔔 Avisos de actualización (`[updates]`)

lazarobox puede avisarte de que hay una versión nueva en crates.io. Cuando existe, la barra de estado muestra `update 0.16.0` y el menú muestra una nota que te indica ir a **Updates**. La fila **Updates** del menú muestra la versión que estás ejecutando (`Updates  0.15.3`); si hay una nueva, el nombre lleva un `*` y el valor dice la versión (`* Updates  0.16.0 available`).

**`Enter` sobre `Updates`** solo actúa si hay una actualización disponible (si no, el pie dice `up to date` y no hace nada):

- **Instalaste con `cargo install`** (el binario en ejecución, con rutas resueltas, incluidos los enlaces simbólicos de `~/.cargo`, está bajo `$CARGO_HOME/bin`, o `~/.cargo/bin` si `CARGO_HOME` no está definido): lazarobox cierra el menú y abre una **pestaña nueva**, que pasa a ser la activa, y ejecuta directamente `cargo install lazarobox-shell --locked --version <versión> --root <raíz>`, sin shell, donde `<versión>` es exactamente la que anuncia la fila Updates (cargo instala exactamente esa versión o falla, en lugar de instalar otra distinta) y `<raíz>` es la carpeta que contiene ese `bin`; así cargo instala siempre encima del binario en ejecución, aunque `CARGO_INSTALL_ROOT` o `install.root` apunten a otro sitio. Cerrar el menú así equivale a pulsar `Esc`: lo que no hayas guardado en el menú se descarta. Es un panel normal: ves la compilación, y sus primeras líneas, escritas por lazarobox, dicen en llano que actualiza, que puede tardar unos minutos y que no cierres lazarobox, y después qué se ejecuta (`running /home/u/.cargo/bin/cargo install lazarobox-shell --locked --version 0.16.2 --root /home/u/.cargo`).
  - **Qué `cargo`:** el que está junto al binario en ejecución (`<CARGO_HOME>/bin/cargo`) y, si no existe, el primero del `PATH` de login que ya usa lazarobox (nunca el directorio actual). Se ejecuta tal como se encontró (el proxy de rustup decide según el nombre con que se lanza). Debe ser de confianza: ni él ni ninguna carpeta por encima pueden estar bajo `/tmp` ni ser escribibles por otros, y el permiso de escritura del grupo solo se acepta (es el caso normal con umask 002) si la carpeta o el archivo es tuyo, su grupo es el tuyo y ese grupo es privado, es decir, se llama como tu usuario y no tiene más miembros que tú. Un grupo compartido (`users`, `staff`) o una carpeta de root con escritura de grupo se rechazan. Se comprueban también las carpetas que contienen el enlace a `cargo` (por ejemplo `~/.cargo/bin`) y se vuelve a comprobar al pulsar `Enter`, por si algo cambió desde el arranque. Es una regla más laxa que la de los CLIs de IA, que sigue exigiendo que ni el grupo pueda escribir. Si no la cumple, no se ejecuta nada y se muestra el comando como en el caso siguiente.
  - **Entorno:** cargo recibe el entorno normal de un panel (el de lazarobox más `TERM`/`COLORTERM`), con el `PATH` de login; no se depura como el de los CLIs de IA, porque cargo necesita tu `RUSTUP_*`, proxy, certificados, etc. Se ejecuta desde tu carpeta personal, no desde la de un proyecto.
  - **Resultado:** si termina bien, la pestaña se cierra sola, un aviso dice `Updated to X.Y.Z. Restart lazarobox to use it.` y la fila pasa a `X.Y.Z installed: restart` hasta que reinicies. Si falla, la pestaña **se queda abierta** con una última línea en llano (`The update did not finish (error N)`, `The update could not start` o `The update stopped unexpectedly`) seguida del comando para ejecutarlo a mano, para que leas el error de cargo; ciérrala cuando quieras (`Ctrl+Espacio`, `w`, `q`). **Reinicia lazarobox** para usar la versión nueva: el proceso en marcha no se reemplaza.
- **Cualquier otra instalación** (paquete de la distribución, tarball, compilación local) o sin `cargo` en el `PATH`: **no se ejecuta nada**. La nota del menú dice `lazarobox cannot update itself here. Run:` con el comando `cargo install lazarobox-shell --locked`, y el pie dice por qué (`installed another way`, `Rust is not installed`, `Rust folder is too open` o `folder name is unusual`); sin Rust, la nota dice antes `Install Rust first (rustup.rs), then run:`. `Enter` o `y` copia el comando al portapapeles (OSC 52).

No hay interruptor: `[updates] check` solo se cambia en el fichero.

**Configuración** (`config.toml`):

```toml
[updates]
check = true   # por defecto; false = ninguna llamada de red y ningún aviso
```

Con `check = false` lazarobox no hace ninguna petición, no lee la caché y no muestra nada. Un valor que no sea `true` o `false` se avisa una vez al arrancar, como cualquier otro problema de configuración, y se usa el valor por defecto.

**Qué se envía.** Un único `GET https://crates.io/api/v1/crates/lazarobox-shell`, hecho con el `curl` del sistema (solo HTTPS, sin redirecciones, límite de 5 s), con las cabeceras `User-Agent: lazarobox-shell/<versión> (+https://github.com/pichu2707/lazarobox-shell)` y `Accept: application/json`. Nada más: ni identificadores, ni cookies, ni telemetría. Si `curl` no está instalado, la comprobación no hace nada, en silencio. Las entradas relativas o vacías del `PATH` nunca se usan para encontrar `curl`, `cargo` ni los CLIs de IA. `curl` respeta tu entorno habitual de proxy y certificados (`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, `SSL_CERT_FILE`, `CURL_CA_BUNDLE`); solo se ignora `~/.curlrc`.

**Cada cuánto.** Al arrancar, en segundo plano, y como máximo una vez cada 24 horas mientras la versión cacheada sea más nueva que la que ejecutas. Entre medias se lee la última respuesta de `update-check.json` en el directorio de datos de lazarobox (`$XDG_DATA_HOME/lazarobox/`, o `~/.local/share/lazarobox/`), así que una actualización conocida se sigue mostrando en cada arranque sin nueva petición. La caché guarda qué versión de lazarobox la escribió (`checked_by`). Si la escribió otra versión (o una antigua, sin ese campo) y su versión más nueva es igual o menor que la que ejecutas, por ejemplo tras actualizar, se comprueba en el siguiente arranque sin esperar 24 h; si la escribió esta misma versión, rige el límite de una vez al día. Límite conocido: si dos versiones de lazarobox comparten el mismo directorio de datos y se usan a la vez, cada arranque de la que no escribió la caché puede hacer una petición (como mucho una por arranque; nunca más de una al día para la misma versión). Una comprobación fallida (sin red, tiempo agotado, error de estado, respuesta ilegible) es silenciosa, conserva la versión nueva ya conocida y se reintenta pasada una hora. Si no se puede escribir la caché (directorio de solo lectura), no se comprueba nada, para no consultar la API en cada arranque. Las versiones preliminares nunca se anuncian.

**Comprobar ahora.** Sobre la fila **Updates**, `r` pregunta a crates.io en ese momento, sin mirar la caché ni el límite de 24 horas. Mientras dura, la fila dice `checking…`; después muestra el resultado como cualquier otra comprobación (y lo guarda en la caché). Una comprobación fallida vuelve en silencio a lo que ya se sabía. Con `check = false`, o tras instalar una versión en esta sesión, `r` no hace nada y el pie no lo ofrece.

---

## 🤖 Menú de IA

Con `Ctrl+Espacio` y `m` se abre el menú; la pestaña **AI** (`Tab`) muestra el motor general (**Set all tasks**) y una fila por tarea. `Enter` sobre una fila de tarea abre su selector de dos niveles (motor y modelo); `Esc` vuelve un nivel. La fila `custom…` permite escribir o pegar el nombre de un modelo. `r` sobre una fila de tarea o dentro del selector vuelve a detectar los motores.

Tras las filas de tarea, la pestaña **AI** tiene las filas **Profile**, **Context**, **Memory**, **Tools**, **Approval** y **Hugging Face** (la última; ver «Descargar modelos de Hugging Face»). **Approval** alterna con `h`/`l` (o las flechas) entre `ask`, `auto` y `run`, el modo de aprobación de los comandos que pide un modelo (`ai.tools.approval`); `s` lo guarda en `[ai.tools]` de `config.toml`, `Esc` lo descarta. El cambio no actúa sobre un chat abierto ni sobre una aprobación pendiente: solo llega a los paneles de chat que abras después y al siguiente cambio de conversación.

`s` guarda la elección en la tabla `[ai]` de `config.toml` (y la de cada tarea en `[ai.tasks.*]`); `Esc` en el nivel superior la descarta. `Enter` nunca guarda ni cierra el menú. Mientras la elección difiera de la guardada, el pie del menú muestra `unsaved changes · s to save · Esc discards`.

El catálogo de modelos de Ollama es solo informativo y puede estar desactualizado. Los modelos del catálogo que no están instalados aparecen como `download required` y no se pueden elegir: lazarobox **nunca descarga un modelo a través de un servidor**. Para usar uno, instálalo tú (`ollama pull <modelo>`) y pulsa `r`. lazarobox solo descarga dos cosas, y solo cuando tú lo confirmas: modelos GGUF de Hugging Face (ver «Descargar modelos de Hugging Face») y, como única excepción a «lazarobox no instala nada», `llama-server` (ver «Instalar llama-server (la excepción)»).

Se detectan estos motores: `ollama`, `lmstudio`, `llamacpp` y `vllm` (servidores locales) y los CLIs `claude`, `codex`, `gemini` y `opencode`. Los servidores se consultan en su puerto habitual (o en el que fijes en `[ai.servers]`, ver abajo) y los CLIs se buscan en el `PATH`. El menú marca como `detected · not in chat yet` los motores detectados que aún no tienen adaptador de chat (hoy solo `opencode`), y no deja elegirlos.

La detección es de solo lectura y solo usa `127.0.0.1`: no se envían credenciales ni datos de tus paneles, y no se ejecuta nada salvo `--version` de los CLIs encontrados.

Si un servidor escucha en otro puerto (por ejemplo, porque el suyo está ocupado), indícalo en `config.toml`. La detección y el chat usan ese mismo puerto, siempre en `127.0.0.1`:

```toml
[ai.servers.llamacpp]
port = 39080
```

Valen `ollama`, `lmstudio`, `llamacpp` y `vllm`; un puerto fuera de 1–65535 o que no sea un número se ignora con un aviso al arrancar y se usa el habitual. El menú solo escribe `managed` y `model` de llamacpp (abajo) y conserva el resto de la tabla al guardar. Un `port` cambiado a mano se aplica al reiniciar lazarobox.

Un servidor instalado (su binario está en el `PATH`) que no responde aparece en el selector como `not running`. Al elegirlo, el menú dice en qué puerto lo busca y cómo arrancarlo, por ejemplo `llamacpp not running on 127.0.0.1:39080 · start it, then press r: llama-server --port 39080 -m <model.gguf>`. Enter o `y` copia esa línea; arranca el servidor, pulsa `r` y aparecen sus modelos. lazarobox no arranca el servidor por ti, salvo el de llama.cpp si se lo pides (abajo).

**llama.cpp gestionado por lazarobox.** Si no quieres tener una pestaña abierta con `llama-server`, lazarobox puede arrancarlo y pararlo por ti:

```toml
[ai.servers.llamacpp]
managed = true
model = "/ruta/al/modelo.gguf"
args = ["-ngl", "99", "-c", "16384"]   # opcional
idle_minutes = 15                       # opcional: 0 = no pararlo nunca
```

**Desde el menú, sin editar nada.** Con `llama-server` instalado pero sin responder (o, si ya está gestionado, aunque responda otro `llama-server` en el puerto), el selector de modelos de llamacpp lista, marcados `lazarobox starts it`, los `.gguf` que encuentra en:

- `~/.local/share/llama.cpp/models` (o `$XDG_DATA_HOME/llama.cpp/models`);
- la caché de llama.cpp: `$LLAMA_CACHE`, o `~/.cache/llama.cpp` (en macOS también `~/Library/Caches/llama.cpp`);
- la caché de Hugging Face: `$HF_HUB_CACHE`, `$HF_HOME/hub` o `~/.cache/huggingface/hub`.

Al elegir uno y guardar, el menú escribe `managed = true` y ese `model` y lazarobox arranca `llama-server` en ese momento, sin reiniciar: la barra de estado dice `starting llama-server with <archivo>…` y luego `llama-server is ready` o por qué no arrancó. De un modelo dividido (`-00001-of-00003.gguf`) solo aparece la primera parte, que carga las demás; los proyectores de visión (`mmproj`) no aparecen. Sin ningún `.gguf` sigue la nota para arrancarlo a mano. `llama-server` carga un solo modelo: elegir otro para cualquier tarea pasa a él las tareas que usaban el anterior, y al guardar se para el servidor anterior y arranca el nuevo. Si cambias de idea y ninguna tarea se queda con el modelo, no se guarda ni se arranca nada.

- En el selector de modelos, el `model` configurado aparece como `lazarobox starts it` aunque el servidor no esté en marcha, y se puede elegir.
- Arrancar lazarobox no lanza nada. El primer mensaje del chat con llamacpp (o guardar el menú tras elegir un modelo, arriba) arranca `llama-server --host 127.0.0.1 --port 39281 -m <modelo>` y espera a que responda (hasta 90 s mientras carga el modelo); los siguientes mensajes reutilizan el mismo servidor.
- Usa el puerto 39281, poco habitual a propósito; `port` lo cambia. Cada modelo tiene su propio servidor: si en ese puerto ya hay otro, lazarobox prueba los siete siguientes (39282-39288).
- `args` añade argumentos a `llama-server` detrás de los que pone lazarobox, por ejemplo `-ngl 99` (capas en la GPU), `-c 16384` (contexto) o `--jinja`. Cada elemento de la lista es un argumento tal cual, sin pasar por ninguna shell (`"--alias", "mi modelo"` son dos). Solo se configura en el archivo; el menú no lo toca. Si la lista incluye algo que fija lazarobox (`--host`, `--port`, `-m` o `--model`, también como `--port=8080`) o no es una lista de textos, se ignora entera con un aviso al arrancar.
- Se para solo tras `idle_minutes` minutos sin usarlo (15 por defecto), para liberar la memoria de la GPU; el siguiente mensaje lo arranca otra vez y espera a que cargue el modelo. Un mensaje que aún se está respondiendo nunca se corta: el tiempo cuenta desde que termina. `0` no lo para nunca. En el menú, la fila `llama-server idle` de Settings pasa por `5 min`, `15 min`, `30 min` y `never` (`h`/`l`) y se guarda con `s`; el cambio vale al momento, sin reiniciar. Un valor del archivo fuera de esos pasos (por ejemplo `45`) se muestra tal cual. Un valor incorrecto se cambia por 15 con un aviso al arrancar.
- Se para al salir de lazarobox, también si cierras la pestaña de la terminal. Si lazarobox se cae o se fuerza su cierre, lo para el vigilante (ver «Al cerrar lazarobox»), también en macOS; en Linux, además, el núcleo lo mata con lazarobox.
- Si en esos puertos ya hay un `llama-server` listo con el mismo archivo de modelo (otro lazarobox abierto, o uno que arrancaste tú en una terminal), lazarobox lo usa tal cual y no arranca nada. Ese servidor no es suyo: no lo para al salir, por inactividad ni al cambiar de modelo; si desaparece, el siguiente mensaje arranca el propio. Solo usa uno cuyo socket sea de tu usuario, para que otro usuario de la máquina no pueda hacerse pasar por él y leer tus mensajes (en Linux lo mira en `/proc/net/tcp`; en macOS, con `lsof`).
- Si en un puerto responde otra cosa (otro modelo, un servidor que aún carga, otro programa), lazarobox no lo toca y arranca el suyo en el primer puerto libre: usar otro modelo, más o menos potente, nunca queda bloqueado por el que ya corre. Si los ocho puertos están ocupados, no arranca nada y el chat dice `llama-server not started: no free port on 127.0.0.1 from 39281 to 39288`.
- Quien arranca un servidor es quien lo para. Mientras responde a otro lazarobox, la parada por inactividad espera y vuelve a contar desde entonces. Al salir, la pregunta lo nombra, `Quit? still running: llama-server (shared) (y/n)`, porque salir corta esa respuesta; el siguiente mensaje del otro arranca un servidor propio. Si en ese momento no responde a nadie, salir no corta nada y no se nombra.
- Las pestañas y los paneles de un mismo lazarobox comparten su servidor: esto solo pasa entre lazarobox distintos, en otra ventana o en otra pestaña de tu terminal con su propio lazarobox.
- Su salida queda en `~/.local/state/lazarobox/llama-server.log` (o `$XDG_STATE_HOME/lazarobox`), que se vacía en cada arranque. Si no arranca, el chat muestra su última línea.
- `model` debe ser una ruta absoluta: `~` no se expande y una ruta relativa se leería desde la carpeta en que arrancaste lazarobox.
- Antes de cada arranque se comprueba que el archivo de `model` existe. Si falta, no se arranca nada y el chat dice `llama-server not started: model file not found: <ruta>`. No se comprueba al cargar la configuración: si el modelo está en un disco que montas más tarde, el siguiente mensaje ya funciona.
- `llama-server` se busca en el `PATH` de login que ya usa lazarobox (nunca en el directorio actual), así que funciona también si abres lazarobox desde el lanzador del escritorio. El nombre del modelo en la tarea del chat da igual (llama-server lo ignora); sin modelo asignado se envía la ruta.
- Un valor incorrecto de `managed` o `model` se ignora con un aviso al arrancar, y el servidor no se gestiona.

Límites de esta fase:

- No se admiten endpoints personalizados: el host es siempre `127.0.0.1`.
- Los alias de shell no se detectan: el binario debe estar en el `PATH`.
- Al arrancar desde un lanzador de escritorio o rofi, el `PATH` suele ser mínimo. lazarobox pregunta una vez a tu shell de login (`$SHELL -l -i -c`; `/bin/sh` si `$SHELL` no está definido) y pone sus carpetas al principio del `PATH`, seguidas de las del proceso que falten, tanto para detectar los CLIs como para lanzarlos en el chat: así se elige el mismo ejecutable que en tu terminal, y una copia antigua en `/usr/local/bin` no tapa la tuya. La shell arranca con el `PATH` del proceso más `/usr/local/bin:/usr/bin:/bin`, para que tus archivos rc (`brew shellenv`, nvm) encuentren `tr`, `readlink` y demás. Si `$SHELL` falla, se prueba una vez `/bin/bash -l -c` (salvo que `$SHELL` ya sea bash; lee los archivos de login pero no `~/.bashrc`); entre las dos consultas comparten un límite total de unos 3 s. Si ambas fallan o tardan demasiado, se usa el `PATH` del proceso tal cual. El entorno del proceso no se modifica. La consulta ejecuta tus archivos rc interactivos al arrancar, así que cualquier demonio que lancen (por ejemplo `ssh-agent`, o un autoarranque de tmux o zellij) también se inicia en ese momento.

### Instalar llama-server (la excepción)

lazarobox no instala programas: detecta los que ya tienes. **`llama-server` es la única excepción**, y solo si tú la pides: con llamacpp en `not installed`, su lista de modelos ofrece `install llama-server…`. Nada se descarga hasta que confirmas cada paso con `y`.

| Tema | Qué hace |
|------|----------|
| Qué se descarga | El archivo `llama-<tag>-bin-…tar.gz` de la última versión de [ggml-org/llama.cpp](https://github.com/ggml-org/llama.cpp/releases) que tiene un build para tu sistema. Eliges la variante: CPU, Vulkan (solo en Linux y solo si hay un driver Vulkan instalado) o Metal (macOS). |
| De dónde | Solo de `github.com/ggml-org/llama.cpp`, con el `curl` del sistema (HTTPS). La URL la construye lazarobox; no se sigue ninguna otra. No se lee ningún token de GitHub. |
| Cómo se verifica | El sha256 del archivo se compara con el `digest` que publica GitHub; si no coincide, se borra y no se extrae nada. Si `gh` está instalado y con sesión iniciada, se ejecuta además `gh attestation verify` y un fallo detiene la instalación; sin `gh` (o sin sesión) el asistente avisa de que la atestación no se verificó y sigue. Antes de activarlo se ejecuta `llama-server --version`; si falla, no se activa. |
| Dónde queda | `$XDG_DATA_HOME/lazarobox/llama.cpp/` (por defecto `~/.local/share/lazarobox/llama.cpp/`): una carpeta por versión y `current` apuntando a la activa. Se conserva la versión anterior para volver atrás a mano. |
| Orden en el `PATH` | La carpeta `current` se añade **al final** del `PATH`: un `llama-server` tuyo (el del sistema) siempre gana. Si ya hay uno, `install llama-server…` no se ofrece. |
| Avisos de versión | Con una instalación propia, al abrir llamacpp se pregunta una vez por ejecución si hay una versión más nueva (de la misma variante que instalaste, también Vulkan) y, si la hay, el pie lo dice, salvo que ya muestre otro aviso. Nunca se instala sola. No se pregunta con `[updates] check = false`, y sin red no dice nada. |
| Cómo quitarlo | Con una instalación propia, la lista de llamacpp ofrece `remove the llama.cpp install`: pregunta `y`/`n` y borra `$XDG_DATA_HOME/lazarobox/llama.cpp/` entero. Si el `llama-server` gestionado está en marcha, se para antes de borrar. Nunca toca un `llama-server` del sistema. También puedes borrar esa carpeta a mano. |

Si prefieres hacerlo tú, la fila `I'll do it myself (copy instructions)` copia al portapapeles cómo hacerlo a mano y no descarga nada. Al terminar, el asistente ofrece descargar un modelo de Hugging Face.

### Descargar modelos de Hugging Face

La lista de modelos de llamacpp termina en la fila `download from Hugging Face…`; sin `llama-server` instalado sigue disponible (la descarga solo necesita `curl`) y avisa de que hace falta `llama-server` para ejecutar el modelo. Desde ahí:

1. **Elige un modelo de la lista.** Al entrar se muestran los modelos de texto GGUF más descargados **que caben en la memoria de tu PC**, con su tamaño estimado (`Qwen/Qwen3-4B-GGUF  630088 ↓  ~2.5 GB`): se eligen con `j`/`k` y `Enter`, sin escribir nada. El tamaño se estima con los parámetros del modelo a unos 5 bits por parámetro (como `Q4_K_M`), y un modelo cabe si ocupa como mucho el 70% de la RAM.
   **O busca.** `Esc` en esa lista abre el campo de búsqueda: escribe un texto para buscar repos GGUF (los más descargados primero) o pega su id, `owner/repo` (por ejemplo `Qwen/Qwen2.5-0.5B-Instruct-GGUF`). Al buscar se muestra todo, y lo que no cabe en tu PC lleva la marca `too big for this PC`.
2. **Elige el archivo.** Se listan solo los `.gguf` con su cuantización y su tamaño (`Q4_K_M  1.1 GB  …`). Un modelo dividido en partes cuenta como uno, con el tamaño de todas. La primera fila dice la licencia.
3. **Confirma.** Antes de bajar nada se muestra qué archivo es, su tamaño, el repo, la **licencia y un enlace para leerla**, la carpeta de destino y el espacio libre. Nada se descarga hasta que pulsas `y`; `n` o `Esc` vuelve atrás. Si no hay espacio, lo dice y `y` no hace nada.
4. **Descarga.** Va en segundo plano, una a la vez. La fila muestra `↓ 42% <archivo> · <licencia>` y la barra de estado `↓ 42%`. `Enter` sobre la fila ofrece cancelarla; lo ya bajado se conserva y, si vuelves a elegir el mismo archivo, sigue donde se quedó. Al terminar se comprueban el tamaño y el sha256 que publica el Hub; un archivo que no coincide se borra.
5. **Úsalo.** Al terminar, el modelo aparece en la misma lista y, al elegirlo, lo arranca el `llama-server` gestionado (ver «llama.cpp gestionado»).

**Dónde se guarda.** En la caché estándar de Hugging Face, la misma que lee la lista de modelos y que usan `hf`, `huggingface_hub` y llama.cpp: `$HF_HUB_CACHE`, o `$HF_HOME/hub`, o `~/.cache/huggingface/hub`, con su estructura habitual (`models--<owner>--<repo>/blobs`, `snapshots`, `refs`). Para borrar un modelo, borra su carpeta `models--<owner>--<repo>` (o usa `hf cache delete`).

**Cuenta de Hugging Face.** Los repos públicos no necesitan cuenta. Para los de acceso restringido o privados, conecta tu cuenta en la última fila de la pestaña AI, `Hugging Face`:

- `Enter` abre un campo donde pegas un token de acceso (se ve como `••••`). Debajo aparece el enlace para crear uno de lectura, `https://huggingface.co/settings/tokens/new?tokenType=read`, en una línea propia para que el terminal lo deje pulsable; `Ctrl+O` lo abre en el navegador, y con `Mouse` activado también basta con hacer clic en esa línea. lazarobox lo comprueba con el Hub y muestra tu usuario. Si el Hub lo rechaza, dice `Hugging Face did not accept the token` y no guarda nada. `Esc` mientras se comprueba la cancela y tampoco guarda el token.
- El token se guarda en el mismo archivo que `hf auth login`: `$HF_TOKEN_PATH`, o `$HF_HOME/token`, o `~/.cache/huggingface/token`, con permisos `0600`. Nunca va a `config.toml`, ni se muestra, ni aparece en la línea de comandos de ningún proceso.
- Con una cuenta conectada, `Enter` ofrece desconectarla (`y/n`), que borra ese archivo. `d` hace lo mismo con cualquier token guardado, aunque no se haya podido comprobar (sin red, o un token que el Hub ya no acepta).
- Si la variable `HF_TOKEN` está definida, manda sobre el archivo, y la fila lo indica: `token set (HF_TOKEN)`.

**Qué se envía.** Solo cuando estás en este flujo (nunca al arrancar ni en segundo plano), con el `curl` del sistema, solo por HTTPS a `huggingface.co` (las descargas siguen sus redirecciones a su CDN, también solo HTTPS):

- la búsqueda (`/api/models?search=…&filter=gguf`) y, al entrar, la lista de los más descargados (`/api/models?filter=gguf&pipeline_tag=text-generation`);
- los datos del repo y su lista de archivos;
- los archivos que confirmas;
- con una cuenta, la comprobación `/api/whoami-v2`.

El token va en la cabecera `Authorization` y solo a `huggingface.co`; `curl` no lo reenvía al CDN. Como en la comprobación de versiones, `curl` respeta tu proxy y tus certificados y se ignora `~/.curlrc`. Sin `curl` en el `PATH`, el flujo dice `Hugging Face needs curl on the PATH`.

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
| `Ctrl+G`               | Abre la revisión de un lote huérfano (insignia `orphan N · Ctrl+G`) |

**Perfiles y prompts:** `profile = "analyst"` o `"programmer"` (por defecto) en `[ai]`, o la fila Profile del menú de IA, elige el _system prompt_ integrado. Puedes sustituirlo con `~/.config/lazarobox/prompts/<perfil>/<tarea>.md` (después `general.md`; máx. 16 KiB). El prompt incluye un bloque de contexto del workspace (proyecto, rama, directorio relativo, SO, shell y fecha; nunca rutas absolutas ni contenido); se desactiva con `context = false` o la fila Context del menú.

**Aviso al terminar:** `notify` en `[ai]` (o la fila Notify de la sección Settings del menú, que recorre los valores con `h`/`l`) avisa cuando una respuesta termina por sí sola y no la estás mirando (otro panel, pestaña o ventana sin foco). Una cancelación tuya nunca avisa. Valores:

| Valor       | Qué hace                                                                                                          |
| ----------- | ----------------------------------------------------------------------------------------------------------------- |
| `"off"`     | Nada (por defecto).                                                                                               |
| `"bell"`    | Solo la campana del terminal (BEL).                                                                               |
| `"desktop"` | Solo una notificación de escritorio con `notify-send`. Si no está disponible o falla, suena la campana.           |
| `"auto"`    | Siempre la campana **y**, si hay `DISPLAY` o `WAYLAND_DISPLAY` y `notify-send` está en el `PATH`, la notificación. |

```toml
[ai]
notify = "auto"
```

Los valores antiguos siguen valiendo: `true` equivale a `"auto"` y `false` a `"off"`. La campana sola depende de cada terminal (el tuyo puede tener el sonido apagado, o tmux / zellij pueden filtrarla), por eso existe la notificación de escritorio.

La notificación de escritorio se ejecuta con `notify-send` (título `lazarobox`; el texto es fijo y solo nombra el panel: `Answer ready in pane N` o `Turn failed in pane N`, nunca la respuesta). Cómo funciona:

- **Se comprueba una vez al arrancar, en segundo plano, y otra vez con `t`.** La comprobación mira la sesión (display, bus), busca `notify-send` y los reproductores de sonido en el `PATH` de login que ya usan los CLIs (nunca el directorio actual), y guarda el resultado. Ningún aviso espera nunca a esa comprobación: si todavía no ha terminado (o falló), el escritorio y el sonido cuentan como no disponibles para ese aviso y, en modo `"desktop"`, suena la campana. Una herramienta instalada después de arrancar se ve tras pulsar `t` o reiniciar. Una comprobación lenta o antigua nunca pisa a una más reciente, y si la comprobación falla por un error interno, `t` lo dice (`the check failed`) y vuelve a comprobar.
- **Sin shell**, con un vector de argumentos, fuera del hilo de la interfaz, sin dejar procesos zombi. Lo que se ejecuta es la ruta real comprobada (con el nombre original como `argv[0]`), y justo antes de ejecutarla se vuelve a comprobar que sigue siendo de confianza; si ya no lo es, cuenta como no disponible.
- **Confianza en el `PATH`:** se ignora un `notify-send` (o un reproductor) bajo `/tmp` o `/var/tmp`, o cuyo fichero o carpeta se pueda escribir por grupo u otros, y se sigue con la siguiente entrada del `PATH`.
- **Entorno depurado:** el proceso hijo solo recibe `PATH`, `HOME`, `USER`, `LOGNAME`, `LANG`, `LANGUAGE`, `LC_*`, `DBUS_SESSION_BUS_ADDRESS`, `DISPLAY`, `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, `XAUTHORITY`, `XDG_CURRENT_DESKTOP`, `XDG_SESSION_TYPE`, `XDG_DATA_DIRS`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CONFIG_DIRS`, `PULSE_SERVER`, `PULSE_RUNTIME_PATH`, `PIPEWIRE_RUNTIME_DIR` y `PIPEWIRE_REMOTE`; ninguna otra variable (claves de API, tokens).
- **Solo con sesión de escritorio:** `DISPLAY` o `WAYLAND_DISPLAY`, y un bus de sesión (`DBUS_SESSION_BUS_ADDRESS` o el socket `$XDG_RUNTIME_DIR/bus`). Por SSH no está disponible: solo si `SSH_CONNECTION` o `SSH_TTY` está definida **y** no hay `WAYLAND_DISPLAY` ni un `DISPLAY` local (`:N`, sin host); así una variable de SSH heredada por un servidor tmux local no desactiva el escritorio, y un `DISPLAY` reenviado (`localhost:10.0`) sí cuenta como remoto.
- **Límites de tiempo:** 2 s por programa (`notify-send` o cada reproductor; pasado el límite se mata y cuenta como fallo), 4 s el envío completo, 12 s la cadena de sonido (cabe cada reproductor, para poder explicar por qué falló cada uno) y 3 s la comprobación de `t`.
- **Una ráfaga no apila procesos:** solo hay un envío en curso a la vez, un sonido a la vez y una prueba a la vez; los que llegan mientras tanto se descartan. En `"auto"` la campana sí suena en cada respuesta; en `"desktop"`, un envío descartado, no disponible o fallido hace sonar la campana (también si la tarea del envío muriera por un fallo interno).
- Al salir, lazarobox espera como máximo 1 s a los hilos bloqueados.

**Sonido propio:** en `"auto"` y `"desktop"`, lazarobox reproduce además un sonido (`[ai] notify_sound = true`, por defecto; fila Sound de la sección Settings del menú), porque un demonio como dunst no suena por sí mismo. Prueba, por orden, `canberra-gtk-play -i message-new-instant`, después `pw-play <fichero>` y después `paplay <fichero>`, con cada uno de los ficheros que existan de `/usr/share/sounds/freedesktop/stereo/message-new-instant.oga` y `complete.oga`. Solo cuenta como éxito una salida con código 0 dentro de los 2 s: un reproductor que sigue ejecutándose pasado el límite se mata y se pasa al siguiente candidato (igual que ante un error al arrancar o una salida con fallo). Si ninguno funciona, no hace nada. En modo `"desktop"`, si `notify-send` no está disponible suena la campana y el sonido propio también puede sonar: se solapan, es esperado. Mismas reglas que `notify-send`: sin shell, entorno depurado, confianza del `PATH` y nada por SSH o sin sesión.

**Probar la configuración:** en la fila Notify (o Sound) de la sección Settings del menú, `t` vuelve a comprobar la máquina y lanza ahora mismo un aviso con el modo y el sonido que muestra la fila (aunque no estén guardados y sin importar el foco; el texto es `Test notification`). Solo una prueba a la vez (mientras corre, el pie dice `test running`). El pie de esas filas (hasta la siguiente tecla) dice qué pasó: `sent: bell + desktop + sound (pw-play)` (con el reproductor que funcionó), o por qué no, por ejemplo `desktop unavailable: no notify-send on PATH`, `no session bus`, `SSH session`, `notify-send on PATH is untrusted (...)` o `no sound: no player worked (canberra-gtk-play: still running after 2 s, killed, ...)`.

Para cerrar el panel: `Ctrl+Espacio`, `w`, `q`. La respuesta se muestra mientras se genera; si el modelo "piensa" (por ejemplo `qwen3`), la barra de estado muestra un indicador `thinking` y el razonamiento queda en un bloque aparte.

**Motor y modelo:** el chat usa la tarea `chat`. Se elige en el menú de IA (`Ctrl+Espacio` y `m`) o a mano en `config.toml`:

```toml
[ai.tasks.chat]
engine = "ollama"
model = "llama3.2"   # o cualquier modelo que tengas descargado
```

Si la tarea `chat` no tiene motor propio, usa el motor general (`[ai]`). El panel indica qué falta cuando no hay motor, no está en marcha o no está soportado.

**Compactación del historial:** cuando un chat largo desborda la ventana del modelo, lazarobox resume lo que sale con un modelo y lo envía como bloque inerte en lugar de perderlo. El resumen nunca va a un motor que no hayas elegido: usa la tarea `summarize` solo si la fijas explícitamente en `[ai.tasks.summarize]`; si no, usa el motor y el modelo del chat (nunca el general). Cuesta una llamada extra cada pocos envíos; `[ai.chat] compaction = "off"` la desactiva y queda el aviso de que los mensajes antiguos no se envían (por defecto `"auto"`; cualquier otro valor es un error de configuración).

```toml
[ai.chat]
compaction = "off"   # "auto" (por defecto) | "off"
```

**Privacidad:**

- Con **Ollama**, **LM Studio**, **llama.cpp** y **vLLM** todo es local: el chat solo habla con `127.0.0.1` en el puerto habitual de cada servidor (11434, 1234, 8080 y 8000, o el de `[ai.servers]`) y no envía credenciales (la memoria opcional de engram tiene su propia sección más abajo).
- Con **claude, codex y gemini** la conversación (tus mensajes, el prompt del perfil y, si la activas, la memoria del proyecto) **se envía al proveedor** del CLI, con tu cuenta y bajo sus condiciones. lazarobox no lee ni copia tus credenciales.
- Las conversaciones se guardan en un `chat.db` **sin cifrar**, texto plano con permisos `0600`, en `$XDG_DATA_HOME/lazarobox/chat.db` (por defecto `~/.local/share/lazarobox/chat.db`). Para borrarlas todas, cierra lazarobox y elimina ese archivo (y los auxiliares `chat.db-journal` si existen). Una conversación se borra por separado desde la lista (`d`, luego `y`).
- El razonamiento del modelo se muestra en el panel pero **nunca** se guarda en `chat.db` (ver «Historial de conversaciones»).

Límites de esta versión:

- `opencode` se detecta, pero aún no tiene adaptador de chat. El menú lo marca con `detected · not in chat yet` y no deja elegirlo.
- El chat con `llama.cpp` (`llama-server`) está probado con respuestas grabadas de un servidor real; `lmstudio` y `vllm` usan el mismo formato (`/v1/chat/completions` con SSE) pero aún no se han grabado ni probado con un servidor real. Ninguno de los tres muestra razonamiento ni ofrece herramientas al modelo.
- Las etiquetas extra de una conversación vuelven en orden alfabético al reabrirla.

### Run a command from the chat

When the last answer of a chat ends with a shell code block (` ```bash `, `sh`, `shell` or `zsh`), a hint under the block names the binding: `Ctrl+Space`, then `e`. It works with every engine, because it reads the block of the answer and needs nothing from the model.

`e` opens a confirmation over the composer: `run in pane N (<cwd>)`, the exact command and the keys. The target is the shell pane of the current tab that you used last; a chat or a pane of another tab is never a target.

| Key       | Action                                                                 |
| :-------- | :--------------------------------------------------------------------- |
| `y`       | Types the command into the target pane. **It never presses Enter.**   |
| `n` / `Esc` | Declines. Nothing is written.                                        |
| `p`       | Moves the target to the next shell pane of the tab (with wrap).        |
| `Ctrl+C`  | Cancels the answer in progress and declines.                           |

Every other key is swallowed while the confirmation is open, and a held key or a paste never answers it. You review the typed line in your own shell and run it yourself with Enter. A multi-line command is sent as one bracketed paste, so the shell shows it as an editable buffer; if the shell did not ask for bracketed paste, a multi-line command is refused. A pane that runs a full-screen app (an editor, a pager) is refused, and the command is checked again when you press `y`: control characters and escape sequences make it be refused, never cleaned.

There is no "always allow". To turn the feature off, set `enabled = false` in `[ai.tools]` of `config.toml`, or flip the **Tools** row of the AI menu (on by default); the hint and the binding then disappear. The menu writes only `ai.tools.enabled` and `ai.tools.approval`; it never touches `[ai.mcp.*]`.

**Which paths the read tools refuse.** Anything outside `$HOME`, `~/.ssh`, `~/.gnupg`, the credential folders of other CLIs (and `~/.password-store`), `~/.config/gh`, `~/.config/gcloud`, the keyrings in `~/.local/share/keyrings`, lazarobox's own folders `~/.config/lazarobox` (its `config.toml` holds MCP env tokens) and `~/.local/share/lazarobox` (chat history, engram, update cache), and, at any depth, `.env`, `.env.*`, `.envrc`, `.netrc`, `.git-credentials`, `.pgpass`, `.npmrc` and the shell or REPL histories (`.bash_history`, `.zsh_history`, `.zhistory`, fish's `fish_history`, `.python_history`, `.node_repl_history`, `.psql_history`, `.mysql_history`, `.sqlite_history`). `.envrc` and `.npmrc` are denied at any depth on purpose, because a project file may hold a token. The list is best effort, not complete: a secret in a file with another name is not detected, and folders moved with `GH_CONFIG_DIR` are not followed. lazarobox's own config and data folders are passed to the policy by the tool runtime, so they follow `XDG_CONFIG_HOME` and `XDG_DATA_HOME`. The roots are read once per call, so a secret folder created while a call runs is not covered by that call.

**Names left out.** `list_dir` and `find_dir` leave out any name, or any joined path in `find_dir`, that the secret redaction would change (`sk-...` keys, `Bearer ...`, `token=...` and the like), so a listing never shows half a key and a query cannot probe for one. A harmless name can match too (`sk-learn-extras`). When anything was left out the result ends with `[some names omitted: they look like secrets]`, whatever the query was.

Links do not get around it: the path is judged as requested and as resolved, and the REAL folder behind a secret folder is refused too, so with `~/.ssh -> ~/dotfiles/ssh` neither `~/.ssh/id` nor `~/dotfiles/ssh/id` nor a second link such as `~/foo -> ~/dotfiles/ssh` is readable. A link directly in `$HOME` that is named like an excluded file (`.env`, `.netrc`, a history file...) protects its target the same way. Known limit: such a link deeper than `$HOME`, whose target has an ordinary name, is only caught by its requested name; hard links are not detected yet (planned for the tool runtime). A `$HOME` that resolves to `/` or to a system folder (`/home`, `/Users`, `/usr`, `/var`, `/etc`, `/opt`, `/tmp`, `/bin`, `/lib`, `/proc`, `/sys`, `/dev`, `/run`, `/mnt`, `/media`, `/srv`, `/sbin`, `/boot`, `/lib64`, `/private`, `/System`, `/Library`, in any letter case) is refused, and so is every call when `$HOME` or a secret folder cannot be read. `/root` is a valid home. Each argument is limited to 4096 bytes.

```toml
[ai.tools]
enabled = true
approval = "ask"
```

#### Approval mode

By default every command a model asks to run needs your `y` (`approval = "ask"`). In **auto** mode a model's `run_in_pane` is typed into the target pane without a prompt, and **lazarobox still never presses Enter** (salvo en `run`, ver más abajo): the line waits in your shell, and you run it (or clear it) yourself. Reads and everything else keep their usual approval.

- Cycle `ask`, `auto` and `run` for the focused chat with `Ctrl+Space`, then `a` (it also works while an approval is waiting, which stays waiting). A system line in the chat says which mode is on.
- A chat in auto mode wears an amber ` AUTO ` badge, and one in run mode a red ` RUN ` badge, at the top right of its pane, focused or not. Nothing is drawn in `ask` mode, or with tools off.
- `approval = "ask"`, `"auto"` or `"run"` in `[ai.tools]` sets the mode a chat starts with; anything else falls back to `ask` with a notice at startup. The mode of a chat lasts for its conversation: opening another conversation from the list, or starting a new one, goes back to the configured value. The toggle never writes the file, and a changed `config.toml` only reaches panes and conversations opened afterwards.
- The **Approval** row of the AI menu (after **Tools**, before **Hugging Face**) cycles `ask`, `auto` and `run` with `h`/`l` and `s` writes `approval` in `[ai.tools]`, even when it equals the default. It has no live effect: an open chat and a pending approval keep their mode, and the saved value reaches panes opened afterwards and the next conversation switch, like an edit of the file.
- The command is typed through the same checks as `y`: it must pass the command validator, the pane must be a shell of the chat's tab outside a full-screen app, and a multi-line command needs bracketed paste. A check that fails gives the model a `denied` result with the reason and writes nothing. The call row in the chat always shows the exact command.
- The command is only typed when the shell itself is in the foreground of the pane; if a program runs there (`python3`, `ssh`, a nested shell), or the foreground cannot be read, the call is denied (`pane N is running a program`). The pane of an update (`cargo install`) is never a target.
- It falls back to the normal prompt, never to a denial, when: the target pane has the focus (you may be typing in it); the chat's tab is not the one shown, or is zoomed onto another pane (you could not see the typing); a previous auto-typed command still sits unsent on that pane; or five commands were already typed in this turn. The unsent mark clears when you press Enter, `Ctrl+C` or `Ctrl+U` in that pane, paste text with a newline into it while the shell has bracketed paste off (inside bracketed paste a newline runs nothing), or close it. A command you typed with `y` marks its pane the same way. Backspace and other keys leave it set, so after you edit the line the next command still asks.
- An approval already waiting is never answered by the mode, and a call that arrives while one waits is denied (`another approval is pending`).

Residual risks (design D18), so you can decide if auto is for you:

- A typed command lands after a line you left half-typed in an unfocused pane: the typed text is appended to it, and lazarobox only knows about lines it typed itself.
- A program that acts on input without Enter (vi mode of readline or zle in normal state, a pager, a TUI outside the alternate screen) can react to the typed text. Bracketed paste makes readline and zle insert it literally; without it only single lines are typed.
- The foreground check cannot see a shell builtin that reads your input (`read`, `select`): the shell is still the foreground process, so the text is typed into what the builtin is waiting for.
- The foreground check happens just before the write; a program started in those microseconds is not covered.
- A model can still type a destructive command that you then run by reflex. The badge, the tool row with the exact command and the system line are the mitigations, and `ask` stays the default.

#### Modo `run`

En modo **run** lazarobox hace lo que `auto` no hace: para un comando de una lista corta y segura escribe el comando **y pulsa Enter**, espera a que termine y devuelve su salida al modelo. Cualquier otro comando sigue el camino normal: aparece la confirmación y `y` solo lo escribe, sin Enter. Es opt-in (`approval = "run"` o el ciclo `Ctrl+Space`, `a`) y `ask` sigue siendo el valor por defecto.

**Lista segura.** Un único comando, con palabras separadas por un solo espacio, de entre `cd`, `pwd`, `ls`, `tree` y `mkdir`:

- `cd` (como mucho una ruta), `pwd` (ninguna), `ls` (flags `l a h t r 1`, hasta 4 rutas), `tree` (`-d`, `-L <n>`, una ruta; sin `-a`, para no listar nombres de archivos ocultos) y `mkdir` (`-p`, de 1 a 4 rutas).
- Las palabras solo admiten letras latinas (también acentuadas), dígitos y `. _ / @ % + : , = ~ -`; el espacio solo va dentro de comillas. Cualquier otra cosa pregunta, por ejemplo cirílico, CJK, emoji, `*` o `;`.
- Las rutas pueden ir sin comillas o entre comillas simples o dobles (`ls -la "Mis Documentos"`): una palabra entera entre comillas, sin `$`, comilla invertida, `\`, `!` ni otras comillas. El verbo nunca va entre comillas.
- Solo las rutas de `mkdir` y `tree` deben resolverse dentro de `$HOME`. Con todos los verbos salvo `cd` se comprueba siempre el directorio actual del panel; para `tree` debe estar dentro de `$HOME` aunque se le dé una ruta. `cd` y `ls` pueden ir a cualquier sitio (`cd /tmp`, `ls /etc`), salvo las carpetas de secretos (`~/.ssh`, `~/.gnupg`, credenciales…), también si se llega a ellas por un enlace simbólico. Lo que no cumpla esto, o no exista, pregunta y la confirmación dice por qué.
- No entran `&&`, `;`, `|`, `$`, redirecciones, globs ni opciones largas. Las rutas pasan la misma política que las herramientas de lectura, comprobada sobre el directorio real del panel.
- Además, el panel debe estar libre: sin comando en ejecución, sin línea a medias, sin el foco, en la pestaña visible y con el locale en UTF-8. Si no, se usa la confirmación normal. Cada turno admite como máximo 5 comandos automáticos, y tres rechazos seguidos por gramática o ruta pausan el modo `run` hasta el final del turno; el contador vuelve a cero cuando un comando se ejecuta y en cada turno nuevo, y los motivos del estado del panel (ocupado, foco, línea a medias…) no cuentan.

**Por qué pide confirmación.** La confirmación de un comando que no corrió directamente muestra una línea con el motivo y el trozo que lo causó, por ejemplo ``shell metacharacter: `&&` ``, ``not a safe command: `git` `` o `a command is still running in the pane`. Los caracteres de control, de formato (bidi, espacio de ancho cero) e invisibles se dibujan como `?`. La línea solo aparece si la confirmación tiene una fila libre; el comando completo siempre se muestra.

**Salida que ve el modelo.** El modelo recibe el texto que quedó en pantalla tras el comando, no un flujo de la shell:

- Máximo 8 KiB y 200 filas: se conservan el principio y el final y se marca el recorte. Pasa por la misma limpieza y redacción de secretos que las herramientas de lectura.
- Un `cd` informa además del directorio nuevo. Un comando que no termina en 10 s devuelve `still running` con lo visible hasta entonces. El resultado nunca incluye el código de salida.
- Si la shell reescribe lo escrito (por ejemplo las abreviaturas de **fish**), no se reconoce el eco del comando y el resultado es `output not available`.
- Si tu prompt ocupa varias líneas, las superiores pueden colarse en la salida que recibe el modelo (se redactan, pero no se detectan).
- Un programa a pantalla completa, que escribas en el panel mientras corre, o un panel que se cierra, tampoco devuelven salida.

**Privacidad.** En `run` la salida del panel **se envía al modelo**, también con motores en la nube (claude, codex, gemini): puede incluir nombres de archivos, rutas y lo que imprima el comando. En `ask` y `auto` el modelo no recibe nada de la salida ni del directorio. Si algo no debe salir de tu máquina, no uses `run` con esa conversación.

Riesgos residuales (diseño D20/D21 de `auto-execute`), que se suman a los de `auto`:

- Hooks de `cd` (`chpwd`, direnv, un prompt que ejecuta `git status`) corren en tu shell al cambiar de directorio. `CDPATH` y `cdable_vars` pueden mandar `cd nombre` a otro sitio.
- Entre la comprobación de la ruta y la escritura hay una ventana en la que un enlace simbólico puede cambiarse (TOCTOU). Se comprueba de nuevo justo antes de escribir, pero no se elimina.
- Continuación del prompt (PS2): si dejaste abierta una comilla, un `$(`, un heredoc o un bucle, o una línea que acaba en `\`, `&&`, `|`, `do` o `then`, el comando del modelo puede unirse a lo tuyo. Las palabras entre comillas son texto inerte y lazarobox rechaza texto con comillas sin cerrar, pero el caso en que lo cierras tú no se detecta hasta tener marcas de prompt (OSC 133).
- Una línea de prompt que se cuele en la salida, como se dijo arriba.
- Fuera de `$HOME`, `cd` y `ls` exponen solo nombres y los metadatos de `ls -l` (incluidos los destinos de enlaces simbólicos), nunca el contenido de los archivos. Los montajes bind no se detectan.
- El locale se lee del entorno de lazarobox; un archivo rc que lo cambie después no se ve.
- Todo lo que dice `auto` sobre líneas a medias, `read`/`select` y programas que reaccionan sin Enter sigue valiendo.

### Motores CLI: claude, codex y gemini

El chat puede hablar con los CLIs **claude**, **codex** y **gemini** que ya tengas instalados y con la sesión iniciada (`claude`, `codex login`, `gemini`). lazarobox no pide ni guarda claves de API: arranca el CLI, le pasa la conversación por la entrada estándar y lee su respuesta. Se eligen como cualquier otro motor, en el menú de IA o en `[ai.tasks.chat]`.

Versiones comprobadas: claude 2.1.299, codex-cli 0.142.5 y gemini 0.63.0. Otras versiones pueden fallar (ver «Si un CLI cambia»).

**claude necesita la versión 2.1.259 o posterior**, porque las opciones de solo texto que usa lazarobox (`--permission-prompts none` entre ellas) no existen en versiones anteriores. Con una versión menor, lazarobox no lanza claude: el menú marca la fila con `update needed: 2.1.259+` y el chat muestra cómo actualizar (`claude update`, `brew upgrade claude-code` o `npm i -g @anthropic-ai/claude-code@latest`); después, `Ctrl+Espacio` `m` y `r` vuelven a detectar la versión. Si no se puede leer la versión, claude se lanza igualmente. Nunca se quitan opciones de solo texto para que quepa un claude antiguo.

**Solo texto.** El chat es de texto: el CLI se lanza con opciones fijas que le quitan las herramientas, de modo que no ejecuta comandos, no edita archivos ni usa servidores MCP. Pedirle «crea un archivo» no crea nada. Es la primera capa; la segunda es la jaula.

**La jaula (Landlock en Linux, Seatbelt en macOS).** Cada turno de un CLI se ejecuta dentro de una jaula del núcleo (Landlock, ABI 3 o superior; lazarobox se relanza a sí mismo para aplicarla antes de arrancar el CLI). La jaula **bloquea**:

- la lectura de todo lo que no se haya concedido: tu `$HOME`, `~/.ssh`, `~/.gnupg`, las carpetas de otros CLIs (`codex` no puede leer `~/.claude`) y `/tmp`;
- la escritura fuera de la carpeta de estado del propio CLI (`~/.claude` y `~/.claude.json`, `$CODEX_HOME` o `~/.codex`, `~/.gemini`) y de un directorio temporal privado de cada turno;
- la ejecución de programas: solo se puede ejecutar el intérprete (node) y el propio CLI. `ssh`, `curl`, `bash` o `git` no se pueden lanzar directamente;
- abrir puertos TCP para escuchar (desde Landlock ABI 4).

El directorio de trabajo del CLI es una carpeta vacía y privada, el entorno se limpia (`SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, agentes y variables de otras sesiones) y no se usa ninguna shell.

**Disponibilidad por sistema:**

- **codex y gemini** exigen la jaula. Sin ella (otro sistema operativo, un núcleo sin Landlock o una instalación que la jaula rechaza) no se lanza ningún proceso y el panel indica `<motor>: not available on this system yet: <motivo>`. Una instalación que la jaula rechaza no se detecta en el panel: se ve al enviar, como el error `the CLI did not start: its jail could not be enforced: <motivo>`, y en la lista de motores del menú como `no jail: <motivo>`.
- **claude** funciona también sin la jaula: en ese caso el panel avisa una vez con `claude runs without the jail here: <motivo>`. En ese modo solo lo protegen las opciones de solo texto.
- En sistemas que no son Linux ni macOS no hay jaula. El chat funciona con claude (sin la jaula, protegido solo por sus opciones de solo texto) y con Ollama, mientras que codex y gemini no arrancan. El mensaje lo indica: `the CLI jail needs Linux or macOS; on this system use claude or Ollama for chat`.
- **macOS: codex y gemini corren dentro de la jaula (Seatbelt).** Se aplica con `/usr/bin/sandbox-exec`, con el mismo reparto de permisos que en Linux (ver abajo). macOS 14 o posterior es la versión admitida; una versión anterior no se bloquea si la comprobación inicial pasa, pero no está probada. Al arrancar, lazarobox comprueba con tres programas de prueba (uno permitido, una escritura denegada y una ejecución denegada) que la jaula de verdad restringe, con un único límite total de 10 s. Si falla, codex y gemini se rechazan, nunca se ejecutan sin jaula.
- **gemini en macOS: todavía no verificado en un Mac real.** Se prueba en CI con CLIs de mentira, pero no con el gemini real. codex se está verificando con un probador (ver `docs/macos-tester-checklist.md`).
- **claude nunca corre dentro de la jaula en macOS.** Guarda su sesión en el llavero (Keychain) y la jaula se lo impediría; en macOS siempre corre sin jaula, solo con sus opciones de solo texto, aunque la comprobación de la jaula falle. El panel lo avisa con `claude runs without the jail here: claude keeps its macOS Keychain login, so it never runs jailed on macOS` y nunca lo trata como «jaula no disponible».

**La jaula en macOS: lo que cambia.**

- Las lecturas de sistema (`/usr/lib`, `/System/Library`, `/private/etc`, los prefijos de Homebrew…) se conceden en solo lectura, porque dyld las necesita. El `$HOME` sigue cerrado salvo la carpeta de estado del propio CLI y la carpeta temporal privada del turno.
- **Lista de carpetas del `$HOME` que nunca se conceden** (además de `~/.ssh`, `~/.gnupg` y las carpetas de otros CLIs): todo `~/Library`, es decir, la propia carpeta y todo lo que hay dentro (`Keychains`, `Application Support`, `Caches`, `Preferences`, `Cookies`, `Mail`, `Messages`, `Safari`, `Containers`, `Group Containers`, `Mobile Documents`, `Accounts`, `Calendars`, los datos de Chrome o de 1Password, `Developer`...). La única excepción es una lista corta de lugares de instalación: hoy solo `~/Library/Application Support/fnm` (las versiones de node de fnm). Se juzga la ruta tal como se pide y ya resuelta, y las carpetas de credenciales siguen rechazadas aunque cayeran bajo una excepción.
- **Qué abre la jaula para TLS:** la búsqueda mach de `com.apple.SecurityServer` (el demonio de confianza y llavero). codex la necesita para verificar los certificados TLS a través de Security.framework; sin ella no llega a `api.openai.com`. Es una búsqueda mach hacia `securityd`, que lee los llaveros fuera de la jaula y puede servir ELEMENTOS del llavero por ese puerto, con sus propias ACL y avisos; los ficheros del llavero (`~/Library/Keychains`) siguen denegados por el perfil, pero el acceso por el servicio no. Es un riesgo aceptado.
- Una disposición donde un programa ejecutable cae dentro de una carpeta escribible (o al revés) se rechaza, porque Seatbelt suma las reglas y el CLI podría ejecutar lo que escribe.
- Un CLI de scripts con `#!/usr/bin/env node` recibe además el permiso de ejecutar `/usr/bin/env`, y un `node` de Homebrew puede leer su `openssl.cnf`.

**Riesgos aceptados en macOS:** (1) el servicio `com.apple.SecurityServer` descrito arriba. (2) La red de la jaula de macOS está abierta hacia fuera, así que un CLI enjaulado puede alcanzar `localhost` (loopback), tu red local (LAN) y la dirección de metadatos de la nube (169.254.169.254), igual que cualquier otro destino. Es lo que necesita para hablar con su proveedor; abrir puertos para escuchar sí está bloqueado.

**Riesgos que quedan (léelos):**

- **La jaula limita, no es un sello.** Landlock no controla `mmap` con permiso de ejecución, así que el cargador dinámico que necesita un CLI de scripts podría arrancar un programa legible; por eso se retiran de la lectura `/usr/bin`, `/usr/sbin` y `/bin`. Los programas que quedan bajo `/usr/lib` siguen siendo alcanzables. Las restricciones de `execve` son defensa en profundidad. La red **no** está restringida: un CLI puede conectar a cualquier destino (es lo que necesita para hablar con su proveedor).
- **Las carpetas de estado son escribibles.** El CLI debe poder renovar su sesión, así que `~/.claude`, `$CODEX_HOME`/`~/.codex` y `~/.gemini` están en modo escritura. Si un modelo consiguiera que el CLI escribiera ahí una configuración (por ejemplo, hooks o servidores MCP), una ejecución posterior de ese CLI **sin** la jaula (tú, en tu terminal) la ejecutaría con tus permisos.
- **claude reescribe `~/.claude.json` en el sitio.** Dentro de la jaula no puede crear el archivo temporal junto a `~/.claude.json`, y escribe directamente sobre él. Un corte en mitad de la escritura, o dos claude a la vez, pueden dejarlo dañado. Se está estudiando `CLAUDE_CONFIG_DIR` para evitarlo.
- **El _system prompt_ de claude se ve en `ps`.** claude recibe el prompt del perfil como argumento de línea de comandos, visible para cualquier proceso local mientras dura el turno. codex y gemini lo reciben por la entrada estándar y en `ps` solo se ven opciones fijas y el nombre del modelo.
- **La respuesta es la palabra del modelo.** Los modelos pueden negarse o inventar una justificación («no tengo permisos»). La garantía real está en la ausencia de herramientas y en la jaula, no en lo que diga el modelo.
- **Comprobación pendiente:** la línea base de llamadas al sistema con `strace` (M12/M14) no se ha tomado porque `strace` no estaba instalado. En su lugar se midió qué programas arranca cada turno (solo node y el CLI nativo) y que el turno funciona con la jaula completa.

**claude.** Usa `--tools ""`, sin servidores MCP, sin comandos con barra y sin guardar sesión (`--no-session-persistence`). El prompt del perfil sustituye al prompt integrado del CLI.

**Herramientas con claude (Tools with claude).** **Activas** desde que el usuario pasó la comprobación manual el 2026-10-17: claude figura en `MCP_VERIFIED` (la lista de motores cuya comprobación manual pasó). Con `[ai.tools] enabled` activo, cada turno hace esto.

Comprobado con un claude 2.1.302 real y un servidor MCP de prueba: `--tools ""` no oculta las herramientas MCP, `--allowedTools=mcp__lazarobox` las permite y el `tool_use` llega como `assistant.message.content[]` con `name` `mcp__lazarobox__<herramienta>`. El usuario comprobó a mano el efecto de `MCP_TOOL_TIMEOUT` y la llamada completa dentro de lazarobox el 2026-10-17.

- lazarobox abre un socket Unix privado por chat (carpeta 0700, socket 0600) y escribe en la carpeta temporal privada del turno un `mcp.json` (0600) que lanza `lazarobox __mcp-serve <socket>` como servidor MCP `lazarobox`. El token de la sesión va **solo** en la sección `env` de ese archivo, nunca en los argumentos (`ps` los muestra) ni en el entorno de claude; la carpeta se borra al acabar el turno.
- claude recibe `--mcp-config=<mcp.json>` y `--allowedTools=mcp__lazarobox` (con `=`: ambas opciones aceptan varios valores y, sin él, se tragarían lo que venga detrás), mantiene `--tools ""` y `--strict-mcp-config` (ningún otro servidor, ninguna herramienta propia) y `MCP_TOOL_TIMEOUT=900000` (15 min, para que esperar tu `y` no corte la llamada).
- Un turno con herramientas cambia además `--safe-mode` por `--setting-sources=` (vacío): `--safe-mode` desactiva también los servidores MCP, así que con él claude no vería las herramientas. Con `--setting-sources=` no se leen tus ajustes de usuario, de proyecto ni locales, ni habilidades, _hooks_ o comandos, y se carga solo nuestro servidor; los ajustes gestionados o de empresa (_managed_) podrían seguir aplicándose. Los turnos de solo texto, y el reintento sin herramientas, mantienen `--safe-mode`.
- El _prompt_ de sistema cambia **solo en un turno que de verdad corre con las herramientas conectadas**: en vez de decir que no tienes acceso a nada, cuenta que hay herramientas (buscar un directorio, listar, leer un archivo y escribir un comando en el panel de shell, que solo se teclea: tú lo revisas y pulsas Enter) y que sigue sin poder cambiar archivos. Si el turno cae a solo texto (sin jaula, sin _shim_, jaula que rechaza la disposición), el _prompt_ es el de siempre. Lo mismo vale para codex y gemini.
- La jaula concede lo mínimo para el _shim_: ejecutar el binario de lazarobox (su ruta canónica, no `/proc/self/exe`, que dentro del CLI es node), su cargador ELF solo si el del CLI no lo cubre, y leer y escribir **el archivo** del socket. No se concede ninguna carpeta, y `ssh`, `curl` o `~/.ssh` siguen bloqueados. El binario y su cargador pasan antes la misma comprobación de confianza que un intérprete (ni el archivo ni su carpeta escribibles por grupo u otros, y fuera de `/tmp`); si no la pasan, el turno sigue como texto.
- Solo cuenta lo que ejecuta lazarobox: con herramientas, el decodificador de claude acepta solo bloques `text`, `thinking`, `redacted_thinking` y `tool_use` llamados `mcp__lazarobox__*` (las filas del chat las pone lazarobox, no se leen de la salida del CLI); cualquier otro tipo de bloque termina el turno con `claude tried to use a tool`. En un chat de solo texto rechaza además cualquier bloque cuyo tipo contenga `tool` o acabe en `_use` (y toda herramienta, también las nuestras, porque no se ofreció ninguna), y tolera tipos de bloque nuevos e inocuos.
- Si algo del cableado falla (no se puede resolver el binario, escribir el archivo, o la jaula rechaza esa disposición), o claude corre sin jaula, **el turno sigue como solo texto** y el chat lo avisa en cada turno que cae a texto: `tools are off for this turn: they could not be set up`. Ese turno cierra también su socket y su token en el acto. La jaula nunca se ensancha para que las herramientas quepan.

**Herramientas con codex (Tools with codex).** **Activas** desde que el usuario pasó la comprobación manual el 2026-10-17: codex figura en `MCP_VERIFIED` (la lista de motores cuya comprobación manual pasó). Con `[ai.tools] enabled` activo, cada turno hace esto.

Comprobado a mano por el usuario con un codex real el 2026-10-17 (2g.6 y 2g.7): los `-c mcp_servers.*` bajo `--ignore-user-config`, las claves `env_vars`, `tool_timeout_sec` y `startup_timeout_sec`, la aprobación de una llamada MCP en `exec` y el elemento `mcp_tool_call`.

- codex recibe el servidor por argumentos `-c mcp_servers.lazarobox.*` (valores TOML): `command` (el binario de lazarobox), `args = ["__mcp-serve", "<socket>"]`, `env_vars = ["LAZAROBOX_MCP_TOKEN"]`, `tool_timeout_sec = 900` (15 min, para que esperar tu `y` no corte la llamada) y `startup_timeout_sec = 20`. No se escribe ningún archivo de configuración.
- El token de la sesión va **solo** en el entorno del propio proceso de codex (`env_vars` solo nombra la variable y codex la pasa al _shim_); nunca en los argumentos (`ps` los muestra), nunca en la salida de depuración de lazarobox y, si el CLI lo repite por stderr, se sustituye por `(..)` antes de mostrar el motivo del fallo. Los argumentos solo llevan rutas, que no son secretos.
- Se mantienen `--ignore-user-config`, el sandbox de solo lectura y las funciones desactivadas (shell y parches siguen apagados). La jaula concede lo mismo que con claude: ejecutar el binario de lazarobox, su cargador si hace falta y leer y escribir **el archivo** del socket, tras la misma comprobación de confianza.
- Solo cuenta lo que ejecuta lazarobox: con herramientas, el decodificador de codex acepta además de `agent_message`, `reasoning`, `error` y `todo_list` un `mcp_tool_call` cuyo `server` sea exactamente `lazarobox`; otro servidor, uno ausente o cualquier otro tipo de elemento termina el turno con `codex tried to use a tool`. En un chat de solo texto no se acepta ninguna herramienta, tampoco la nuestra (el decodificador de codex sigue siendo una lista de tipos permitidos también ahí, a diferencia del de claude).
- Si algo del cableado falla, o codex corre sin jaula, **el turno sigue como solo texto** y el chat lo avisa: `tools are off for this turn: they could not be set up`; ese turno cierra su socket y su token en el acto.

**Herramientas con gemini (Tools with gemini).** **Activas** desde que el usuario pasó la comprobación manual el 2026-10-17: gemini figura en `MCP_VERIFIED` (la lista de motores cuya comprobación manual pasó). Con `[ai.tools] enabled` activo, cada turno hace esto.

Comprobado a mano por el usuario con un gemini real el 2026-10-17 (2h.6 y 2h.7): que gemini lee los ajustes del espacio de trabajo con `--skip-trust` en la carpeta del turno, que la política de administrador gana a la regla que lo deniega todo, el significado de `trust` y `timeout`, y el nombre que gemini da a la herramienta (`mcp_lazarobox_<herramienta>`).

- gemini corre en `<carpeta del turno>/work`, no en la carpeta vacía compartida. Ahí se escribe `.gemini/settings.json` con `mcpServers.lazarobox` (`command`, `args = ["__mcp-serve", "<socket>"]`, `env`, `timeout = 900000` —15 min, para que esperar tu `y` no corte la llamada— y `trust = true`, porque quien te pregunta es lazarobox). Archivo 0600 en carpetas 0700, y todo se borra al terminar el turno.
- gemini solo lee los ajustes del espacio de trabajo si la carpeta es de confianza, y `--skip-trust` llega tarde: por eso un turno con herramientas arranca gemini con `GEMINI_CLI_TRUST_WORKSPACE=true` en el entorno (solo en esos turnos). Tus propios servidores (`~/.gemini/settings.json`) se cargan estén o no en una carpeta de confianza; `--allowed-mcp-server-names=lazarobox` es lo que los deja fuera. Con la carpeta de confianza, el `trust = true` del servidor sí se aplica y gemini no pide su propia confirmación para las herramientas de lazarobox (es lo previsto: pregunta lazarobox). Confiar también habilita más cosas (variables de un `.env` hacia arriba, `GEMINI.md` de carpetas superiores, hooks y skills del proyecto): los ajustes generados las apagan con `advanced.ignoreLocalEnv`, `context.memoryBoundaryMarkers = []`, `hooksConfig.enabled = false` y `skills.enabled = false` (gemini 0.63.0). No hay ajuste para un `.gemini/.env` de una carpeta superior, que gemini lee siempre en una carpeta de confianza; las políticas del espacio de trabajo están desactivadas en el propio gemini 0.63.0. La regla de la política lleva `toolName = "*"` además de `mcpName`: sin él, gemini descarta el archivo por inválido.
- Se añaden `--allowed-mcp-server-names=lazarobox` y `--admin-policy=<archivo>` en la forma con `=`, porque ambas opciones aceptan varios valores y un valor suelto se tragaría lo que viniera después. La política (`mcpName = "lazarobox"`, `allow`) va en el nivel de administrador, que según la tabla de niveles de gemini está por encima de la regla de usuario que lo deniega todo (prioridad 999, que dentro de un mismo archivo no se puede superar). Se mantienen `-e none`, `--approval-mode plan` y esa regla: cualquier otra herramienta sigue denegada.
- El token de la sesión va **solo** en el `env` de ese `settings.json`; nunca en los argumentos (`ps` los muestra), nunca en la salida de depuración de lazarobox y, si el CLI lo repite por stderr, se sustituye por `(..)` antes de mostrar el motivo del fallo. La jaula concede lo mismo que con claude y codex: ejecutar el binario de lazarobox, su cargador si hace falta y leer y escribir **el archivo** del socket, tras la misma comprobación de confianza.
- Solo cuenta lo que ejecuta lazarobox: con herramientas, el decodificador de gemini acepta un `tool_use` solo si su `tool_name` es `mcp_lazarobox_<herramienta>`. Así la `read_file` propia de gemini (sin prefijo) no se confunde con la nuestra; cualquier otra herramienta, o un nombre ausente o que no sea texto, termina el turno con `gemini tried to use a tool`. En un chat de solo texto no se acepta ninguna, tampoco la nuestra.
- Si algo del cableado falla (no se puede escribir el archivo, la jaula rechaza esa disposición, el binario no es de confianza), o gemini corre sin jaula, **el turno sigue como solo texto**, en la carpeta vacía de siempre y con los argumentos de siempre, y el chat lo avisa: `tools are off for this turn: they could not be set up`; ese turno cierra su socket y su token en el acto y borra el `work`.

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

#### Troubleshooting the AI CLI messages

Every message below says why a CLI cannot be used. None of them changes what the jail allows: they only report which rule said no.

| Message | What it means and what to do |
| --- | --- |
| `<cli>: not available on this system yet: Landlock ABI 3+ needed (Linux 6.2); this kernel has ABI 2` | codex and gemini need the jail and the kernel is too old. Upgrade to Linux 6.2 or newer. |
| `<cli>: not available on this system yet: Landlock is unavailable or blocked (kernel, container or seccomp)` | The kernel has no Landlock, or something (a container runtime, a seccomp filter, the LSM list) blocks it. |
| `<cli>: not available on this system yet: the CLI jail needs Linux or macOS; on this system use claude or Ollama for chat` | This operating system has no jail, so codex and gemini do not run there. Use claude (it runs without the jail, with only its text-only flags) or Ollama. |
| `<cli>: not available on this system yet: the macOS jail is unavailable: <reason>; on this system use claude or Ollama for chat` | macOS: the startup check of the jail failed (`/usr/bin/sandbox-exec is missing`, `sandbox-exec refused a test program`, `the sandbox did not block a test action`, `the sandbox check timed out`, ...). Nothing was started, and the message you typed stays in the composer. Use claude or Ollama. |
| `the macOS sandbox could not be applied` | macOS: `sandbox-exec` exited with 71 for a turn, so the jail was not in place. It is reported as a jail error, not as a CLI error, and the CLI never runs without the jail. |
| `the CLI did not start: its jail could not be enforced: <reason>` | The jail refused this install or the helper could not apply it. The reason is one of the next rows, or the helper's own line. Nothing was started. |
| `... <path> is writable by group or others, so <node> is not trusted; run: chmod go-w -- '<path>'` | The `node` that runs a script-based CLI (or its folder) can be swapped by other users, so it is not trusted. Run the command, which names the real path that failed. If the folder is not yours (for example `/usr/local/bin`) the message says its owner must fix it (sudo) instead. |
| `... <path> is under a scratch folder; install the interpreter elsewhere` | The interpreter lives under `/tmp` or `/var/tmp`. Install `node` outside of them. |
| `... no node found to run the CLI script; install it next to the CLI or on PATH` | The CLI is a script and no trusted interpreter was found. |
| `... <path> (from the script's #! line) does not exist; reinstall the CLI` | The script names a fixed interpreter path that is gone (a removed node version). Reinstalling the CLI rewrites it. |
| `... <path> is too broad for the jail; keep the CLI and its state in folders of their own` | The CLI, its loader or its state folder (`$CODEX_HOME`, `~/.codex`, ...) is a broad place such as `/usr/local` or `$HOME`. Install the CLI in its own folder. |
| `... cannot read <path>`, `... the program loader <path> is not a regular file`, `... $HOME is not set or not an absolute path`, `... $HOME (<path>) does not exist` | The CLI file, its loader or `$HOME` could not be resolved. Check that they exist. |
| `available · no jail: <short reason>` | In the engine list of the menu (the version gives way to the reason when the row is narrow). The CLI is installed but would not run jailed here (`Landlock ABI 2, needs 3+`, `Linux/macOS only · use claude` (codex and gemini; claude shows `needs Linux or macOS`), `jail unavailable · use claude` (macOS), `<folder> too broad`, `interpreter not trusted`, `no node`, ...). The chat shows the full sentence, and codex and gemini will not start. Press `y` on the row to copy the full reason. |
| `the CLI did not start: its jail could not be enforced` (no reason) | The jail helper failed for a reason outside the fixed set it reports, so none is shown. Update lazarobox and try again; if it persists, report it. |
| `available · version unknown` | The CLI was found, but `<cli> --version` failed, timed out or printed nothing readable. It may still work; try running it in a terminal. |
| `model list unavailable: codex exited <n>: <last stderr line>` | In the model picker. `codex debug models` failed (an old codex without the subcommand, not logged in, ...). Run `codex login` or update codex, then press `r` in the menu. Only `default` and `custom...` are offered meanwhile. The note wraps over up to four rows; on it, Enter or `y` copies the full reason. |
| `model list unavailable: timed out after 4 s` / `output over 2 MiB` | The listing was too slow or too large and was dropped. Press `r` to try again. |
| `model list unavailable: unexpected output format` / `the list is empty` | codex answered but not with a model catalog (a newer or older codex), or with no listed model. |
| `model list unavailable: jail: <reason>` | The listing runs in the same jail as the chat, so the jail rows above apply. |
| `claude runs without the jail here: <reason>` | claude is allowed to run with only its text-only flags; the reason is any row above. |
| `<cli> does not know the option <flag>: this <cli> is older than lazarobox needs; update it ... and try again` / `this <cli> is older than lazarobox needs: it rejected an option; ...` | The installed CLI is too old to know an option lazarobox passes. The flag is named only when it is one lazarobox itself passes, also when the CLI reports it with its value (`--model=<name>` names `--model`, never the value); any other word is not repeated. Update the CLI the way it was installed (`claude update`, `npm i -g @anthropic-ai/claude-code`, `brew upgrade`, ...). lazarobox never drops the option: it is part of what keeps the CLI text-only. |
| `claude <version> is too old for lazarobox (it needs 2.1.259 or newer); update it: claude update, brew upgrade claude-code, or npm i -g @anthropic-ai/claude-code@latest; then refresh from the menu (Ctrl+Space m, then r)` | The detected claude predates `--permission-prompts none` (2.1.259), so its turn is refused before anything starts. Update claude. In the engine list of the menu the row reads `available · update needed: 2.1.259+`. An unreadable version never blocks. |

### Herramientas de búsqueda y escritura (`[ai.tools]`)

Además de leer, un modelo puede buscar en el proyecto y proponer cambios. Cada familia tiene su interruptor en `[ai.tools]`, **todos desactivados por defecto**, y solo actúa si `enabled = true` y el motor del chat tiene las herramientas verificadas:

```toml
[ai.tools]
enabled = true
search = true      # glob, grep y la lectura por ventanas de read_file
write = false      # create_dir, write_file, edit_file, propose_batch
```

La pestaña **AI** del menú muestra las filas **Search**, **Write**, **Exec**, **Git** y **Git write** (`h`/`l` para cambiar, `s` para guardar); **Write**, **Exec** y **Git write** llevan una línea de aviso (`Can change files or run code`). Guardar solo escribe esas claves y conserva el resto de `config.toml`. Con todos los interruptores desactivados, las herramientas son las de siempre. Ollama admite 24 rondas de herramientas por turno cuando alguna de estas familias está activa (8 en caso contrario).

#### Búsqueda: `glob` y `grep`

Con `search = true` el modelo recibe dos herramientas, que trabajan dentro de la carpeta de origen del chat:

- `glob` (`pattern`, y opcionalmente `path`) lista los ficheros cuya ruta relativa casa con un patrón (`*`, `?`, `**`, `[a-z]`). Devuelve solo nombres, ordenados y con un máximo de 100. Nunca pregunta, como `list_dir`.
- `grep` (`pattern`, y opcionalmente `path`) busca una expresión regular en el texto de los ficheros, con un motor de tiempo lineal (sin _look-around_ ni referencias hacia atrás; el patrón admite hasta 512 bytes). Devuelve `ruta:línea:texto`, con un máximo de 100 coincidencias o 20 KiB, y 200 caracteres por línea; el resultado dice cuándo se ha recortado. Con un motor en la nube (claude, codex, gemini) **pide tu `y` en cada llamada**, en cualquier modo de aprobación (`search <patrón> in <carpeta>`); con un motor local se ejecuta sin preguntar.
- Ambas se saltan los enlaces simbólicos, `.git`, `node_modules`, `target`, `dist`, `build`, `.venv` y `__pycache__`, y los ficheros binarios, no UTF-8 o de más de 1 MiB (en `grep`). Los nombres que parecen secretos se omiten y el resultado lo dice; `grep` compara contra el texto ya censurado, así que un patrón no puede servir para sondear un secreto.
- `read_file` admite `start_line` y `max_lines` para leer una ventana de un fichero largo, e indica dónde empieza la siguiente.
- Si hay un lote pendiente de revisar, las tres herramientas ven también los cambios en espera, marcados con `(pending, not on disk)`. El texto de un fichero no puede imitar esa marca.

#### Escritura: lotes con revisión

Con `write = true` el modelo recibe cuatro herramientas. **Ninguna toca el disco** hasta que apruebas el lote:

| Herramienta                    | Qué hace                                                                                       |
| :----------------------------- | :--------------------------------------------------------------------------------------------- |
| `create_dir` (`path`)          | Prepara una carpeta nueva.                                                                     |
| `write_file` (`path`, `content`) | Prepara un fichero entero (hasta 32 KiB): nuevo, o uno existente pequeño que se sustituye.   |
| `edit_file` (`path`, `old`, `new`) | Prepara el cambio de un texto que aparece exactamente una vez (`old` y `new`, hasta 16 KiB cada uno). Edita la versión preparada si la hay. |
| `propose_batch`                | Cierra el lote y abre la revisión. Rechaza un lote vacío o de más de 50 entradas o 1 MiB (pide dividirlo). |

El modelo puede preparar cambios a lo largo de varios turnos antes de proponer. Mientras un lote espera tu respuesta no admite más cambios. Un `write_file` sobre un fichero existente solo se acepta si mide hasta 64 KiB y su texto no cambia al censurar secretos; si no, el modelo debe usar `edit_file`.

**La revisión.** Un lote propuesto abre una vista a panel completo dentro del chat: un fichero nuevo muestra su texto, una edición o sustitución muestra su diff y una carpeta nueva muestra su ruta. Un cambio en un fichero de construcción (`Makefile`, `build.rs`, `Cargo.toml`, `package.json`, `*.sh`, flujos de `.github/workflows`, etc.) lleva un aviso visible. La cabecera muestra cuántas líneas has visto y cuántas faltan.

| Tecla                 | Acción                                                                                              |
| :-------------------- | :-------------------------------------------------------------------------------------------------- |
| `y`                   | Aprueba **todo** el lote. Solo actúa cuando has visto en pantalla todas las líneas (puerta de cobertura) y han pasado 800 ms desde el primer dibujo (armado). Antes, la cabecera dice `y locked` o `y in 0.4s`. |
| `n` / `Esc`           | Rechaza **todo** el lote: no se escribe nada y el modelo recibe `denied`. También esperan los 800 ms de armado, salvo que el lote no se pueda revisar (demasiado grande), donde `n` actúa de inmediato. |
| `Ctrl+C`              | Deniega la llamada (`cancelled`) y cancela el turno. El lote se queda como huérfano (ver abajo).    |
| `↑` `↓` / `j` `k`, `PgUp` `PgDn`, `Home` `End`, rueda | Desplazan la revisión. Saltar con `Home`/`End` no cuenta las líneas que se saltan; solo cuenta lo que se dibuja. |

Es todo o nada: no se aprueba un fichero suelto. Una línea que solo está en un panel oculto, en otra pestaña o fuera de pantalla no cuenta como vista. Si ya hay otra aprobación pendiente, `propose_batch` devuelve `denied` (`another approval is pending`).

**Huérfanos.** Si cancelas el turno (o el CLI agota su tiempo) con un lote preparado o propuesto, el lote **se conserva** en el chat. El panel muestra una insignia arriba a la izquierda: ` staged 3 ` mientras el turno sigue vivo y el lote aún no se ha propuesto, y ` orphan 2 · Ctrl+G ` cuando el turno ya no existe (se estrecha a ` O2 ` si falta sitio). Con `Ctrl+G` se abre la revisión del huérfano, con las mismas reglas (cobertura, 800 ms): `y` lo aplica, `n` o `Esc` lo descarta y `Ctrl+C` solo cierra la vista. Un huérfano nunca se aplica mientras un turno de ese panel siga en curso. Al modelo se le avisa, en su siguiente turno, de que aplicaste o descartaste el lote (con cifras, sin rutas).

Cerrar el panel del chat y salir de lazarobox **descartan** los lotes pendientes, con un aviso (`discarded N staged entries`). Nada se guarda: tras reiniciar no queda ningún lote.

#### Modelo de amenazas de la escritura

- **Nada toca el disco antes de `y`.** Preparar, buscar y proponer no crean, cambian ni borran nada; solo el paso de aplicar de un lote aprobado escribe.
- **La raíz de escritura es la carpeta de origen del chat, siempre dentro de `$HOME`.** Se rechaza si el origen falta, es `$HOME`, un ancestro de `$HOME`, una carpeta del sistema o queda fuera de `$HOME`, y cualquier ruta que, tras resolver `..` y enlaces, salga de la raíz. También se rechaza todo lo que esté en `.git/` o `node_modules/` y los nombres de secretos (`.env`, `.npmrc`, `~/.ssh`...). Todos los rechazos devuelven el mismo texto, `path not available`, para no revelar si una ruta existe. Las lecturas de `.git/config` también se rechazan.
- **Reglas de contenido: se rechaza, nunca se limpia.** El texto nuevo no puede tener UTF-8 inválido, caracteres de control, de dirección (_bidi_) o de ancho cero, ni finales de línea CRLF. `edit_file` rechaza un fichero que no sea UTF-8 o tenga CRLF o NUL. Límites: 32 KiB por `write_file`, 16 KiB por campo de `edit_file`, 1 MiB por fichero editado, 50 entradas y 1 MiB por lote.
- **En motores en la nube, `edit_file` pregunta como `read_file`, una vez por fichero y turno.** Las respuestas de `old`/`diff` (existe, no existe, aparece varias veces) revelan qué contiene un fichero, así que el aviso es el mismo que el de `read_file`; si lo deniegas no se prepara nada. Un `read_file` permitido antes del mismo fichero también cubre la edición; un turno nuevo vuelve a preguntar. Un motor local nunca pregunta.
- **El modelo no ve el diff ni la base de la edición.** Las filas del diff y el texto base solo se dibujan para ti; el modelo recibe cifras y rutas (`staged <ruta> (batch: N of 50 entries ...)`, `applied N paths`), nunca contenido del disco.
- **Comprobación previa, diario y deshacer.** Al aprobar, lazarobox vuelve a abrir cada destino sin seguir enlaces (`openat2` con `RESOLVE_BENEATH` y `NO_SYMLINKS`) y compara su contenido con la copia de cuando se preparó. Si algo cambió (_drift_), se rechaza **todo** el lote y no se escribe nada. Después escribe cada fichero con un temporal en la misma carpeta y un renombrado atómico (sin escribir a través de enlaces simbólicos ni de ficheros con enlaces duros), y anota cada paso en un diario. Si un paso falla, deshace los anteriores en orden inverso. Si el deshacer también falla, el resultado es `partial` y lista exactamente las rutas que quedaron cambiadas, para que las revises a mano.
- **Huérfanos y descarte.** Un lote cancelado o con tiempo agotado se conserva como huérfano y solo se aplica si lo revisas con `Ctrl+G` (con cobertura y 800 ms); no se aplica nunca con un turno del mismo panel en curso. Cerrar el panel y salir descartan el lote.
- **Una salida por señal descarta en silencio.** Con `SIGTERM` o `SIGHUP` no se muestra el aviso de descarte, porque el lote vive solo en memoria y no se escribe nada de todas formas.
- Solo Linux: en otras plataformas el aplicar devuelve `writes are not supported on this platform` y no escribe nada.

### Tools with Ollama

Ollama no tiene un CLI que ejecute herramientas, así que lazarobox ejecuta el bucle él mismo sobre `/api/chat`: envía las herramientas en el campo `tools`, ejecuta las llamadas del modelo con el mismo registro, las mismas aprobaciones y el mismo modo (`ask`, `auto`, `run`) que los CLIs, y devuelve los resultados en una nueva petición del mismo turno. En la conversación se ven las mismas filas de herramienta que con cualquier otro motor.

- Como Ollama es local, `find_dir`, `list_dir` y `read_file` se ejecutan sin preguntar; `run_in_pane` siempre espera tu `y` y nunca pulsa Enter.
- Un turno hace como máximo 8 peticiones (24 si hay activa alguna familia de `[ai.tools]`: búsqueda, escritura...). Si el modelo sigue pidiendo herramientas, el turno termina con una línea de sistema y se conserva la respuesta parcial.
- Si el modelo no admite herramientas (el servidor responde con un 400 a la petición con `tools`), se repite una sola vez sin ellas (solo si aún no hubo ninguna ronda; después, un 400 es un error normal) y la respuesta llega como texto normal. El motor y el modelo asignados no cambian.
- Los mensajes de herramienta viven solo en ese turno: no se guardan ni se envían en turnos posteriores. Los límites de tiempo, la sanitización y el tope de la respuesta son los de cualquier turno.
- Solo hay herramientas si **Tools** está activado en el menú de IA (`[ai.tools] enabled`). Si lo desactivas a mitad de un turno, solo se deniegan las llamadas que piden aprobación, igual que con los CLIs.
- **Estado:** las herramientas con Ollama están activas: el usuario las comprobó con un modelo real el 2026-10-17 y `ollama` figura en `MCP_VERIFIED`.
- El límite de tiempo total del turno (15 minutos) incluye el tiempo que pasas decidiendo una aprobación.

### Tus propios servidores MCP (`[[ai.mcp.servers]]`)

Puedes declarar tus propios servidores MCP en el `config.toml` de tu usuario; lazarobox los lanza y ofrece sus herramientas al modelo, con cualquier motor, en los chats con **Tools** activado. Se edita a mano; el menú nunca escribe `ai.mcp.*` y cada guardado conserva esos bytes tal cual, con sus comentarios y sus entradas inválidas.

```toml
[[ai.mcp.servers]]
name = "fs"
command = "mcp-fs"
args = ["--root", "/home/u/dev"]
env = ["GITHUB_TOKEN"]

[[ai.mcp.servers]]
name = "docs"
command = "mcp-docs"
project = "/home/u/dev/mi-proyecto"
```

- `name` (letras minúsculas, dígitos y `-`, de 1 a 24 caracteres, único en su ámbito: la lista global o un proyecto) y `command` (sin shell; cada elemento de `args` es un argumento aparte) son obligatorios. `enabled = false` desactiva la entrada.
- `env` lista solo NOMBRES de variables de tu entorno (hasta 64 bytes), nunca valores: una entrada como `"TOKEN=abc"` o una tabla se rechaza y el valor no aparece en ningún aviso.
- `project` limita el servidor a un proyecto: se aplica solo a los chats cuya raíz git de origen coincide exactamente con esa ruta absoluta, y si el nombre ya existe en la lista global gana la entrada del proyecto; con `enabled = false` esa entrada desactiva el servidor global del mismo nombre solo para ese proyecto (nunca añade uno). Solo se lee tu configuración de usuario: un archivo `.lazarobox/mcp.toml` de un repositorio se ignora siempre, de modo que abrir un repositorio nunca puede añadir un servidor.
- Una entrada inválida se omite entera y se avisa al arrancar (`config: ai.mcp.servers[0].command: expected a non-empty string`); las demás se mantienen. La lista se lee una vez al arrancar.
- Los nombres de las herramientas de un servidor llevan el prefijo `<servidor>__<herramienta>` (cada parte en `[a-z0-9_-]`, hasta 24 caracteres) para no chocar nunca con las herramientas propias. Si dos herramientas darían el mismo nombre, se conserva la primera y se avisa con una línea que nombra el servidor y los dos nombres originales.

#### Cuándo arrancan y qué ves

- Un servidor arranca solo cuando un turno con herramientas lo necesita: al abrir lazarobox, al cargar la configuración, al abrir un repositorio o con **Tools** desactivado no se lanza nada. En ese primer turno lazarobox lo ejecuta (sin shell, con un entorno limpio más solo las variables de `env`, en tu `$HOME`) y le pide su lista de herramientas; no llama a ninguna.
- Cada chat tiene sus servidores: se reutilizan en los turnos siguientes de ese chat y se detienen (grupo de procesos terminado y recogido, sin huérfanos) al cerrar el chat, al hacer un turno sin herramientas y al salir de lazarobox. Una lista de herramientas se lee una sola vez, al arrancar el servidor para ese chat. Un servidor que no pudo arrancar no se reintenta mientras el chat siga abierto: hasta que cierres el chat o hagas un turno sin herramientas, ese chat no lo vuelve a intentar.
- La conversación muestra líneas de sistema: `mcp: started <nombre>` cuando arranca, `mcp: <nombre> unavailable: <motivo>` si no puede (un servidor caído no detiene a los demás ni al turno) y `mcp: <servidor>: <herramienta> dropped, ...` si una herramienta se descarta por nombre repetido o inutilizable. El motivo es una sola línea sanitizada; el stderr del servidor nunca se muestra en bruto.
- Los nombres que empiezan por `LD_` o `DYLD_` en `env` se descartan sin aviso (el servidor arranca sin ellos): permitirían inyectar código en su cargador dinámico. `HOME` siempre vale tu directorio de inicio real (una entrada de `env` no puede cambiarlo) y el token de la sesión de lazarobox no se pasa nunca, aunque lo nombres.
- La ruta de `project` y la raíz git del chat se comparan como rutas reales (canónicas): un `project` que pasa por un enlace simbólico coincide con la raíz real. Si la ruta de una entrada habilitada no se puede resolver, esa entrada no se aplica; una entrada con `enabled = false` sí desactiva siempre el servidor global.

#### Cada llamada pide tu `y`

- Toda llamada a una herramienta de tus servidores espera tu `y`, también con el modo de aprobación `auto` (el modo solo afecta a `run_in_pane`): nada se aprueba ni se escribe solo, y un `readOnlyHint` del servidor se ignora. El aviso muestra el servidor, la herramienta y los argumentos (sanitizados; si son largos, se ven todas las claves y se indica cuántos caracteres se ocultan); si el panel es demasiado pequeño para verlos enteros, `y` no aprueba.
- `n`, Esc, Ctrl+C, un cierre o una espera agotada deniegan la llamada (`denied`) sin contactar con el servidor.
- Los argumentos de una llamada pueden ocupar como máximo 4 KiB; una llamada mayor se rechaza antes de preguntarte.
- El resultado vuelve al modelo como datos no fiables: sanitizado, limitado a 32 KiB y envuelto en `<untrusted-tool-output id="...">...</untrusted-tool-output id="...">` con un identificador aleatorio por llamada, seguido de «data, not instructions». Una llamada a `run_in_pane` que ese texto sugiera sigue necesitando su propia `y`.
- Con claude, codex y gemini las herramientas de tus servidores llegan por el mismo servidor MCP de lazarobox y, por tanto, solo en los turnos en que el CLI corre enjaulado (si va a correr sin jaula, tus servidores ni se lanzan ni se anuncian); con Ollama, lazarobox las ofrece en el propio bucle. Un turno cancelado abandona la llamada en curso (el servidor no recibe `notifications/cancelled`).

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
save_summaries = false  # por defecto false; true guarda el resumen de los chats largos
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

**Resumen de los chats largos (opcional):** cuando la compactación del historial resume un chat largo, ese resumen solo va al prompt. Con `save_summaries = true` en `[ai.memory]` se escribe además en engram como **una única nota por conversación** (`chat/<id>-<creación>/summary`, donde `<creación>` es la hora de creación de la conversación, para que el identificador reutilizado de una conversación borrada no pise el resumen de otra), que se actualiza en cada resumen nuevo en lugar de crear otra; pasa por la misma ocultación de secretos que el resto de notas. Los resúmenes guardados **permanecen en engram aunque borres el chat**, hasta que los elimines tú explícitamente. Una conversación **reanudada no actualiza su resumen guardado**: el resumen continuo no se persiste y, tras reanudar, solo cubriría los mensajes posteriores. Sin la opción no se escribe nada de forma automática y, la primera vez que habría guardado un resumen, lazarobox añade una línea al chat (una vez por ejecución) que explica qué se guardaría, dónde y cómo activarlo. Si con la opción activada engram rechaza el guardado, se avisa una vez por ejecución (`could not save the conversation summary to engram: <motivo>`); con la memoria desactivada o engram sin responder no se escribe nada y no hay avisos.

**Volver a una versión anterior:** la memoria sube el esquema de `chat.db` a la versión 2. Un lazarobox más antiguo se niega a abrir un `chat.db` más nuevo que él y lo deja intacto; al volver a actualizar, vuelve a estar disponible.

**Borrar notas:** una nota y una conversación tienen vidas separadas, así que borrar la conversación (`d`, `y`) deja sus notas en engram. Si la memoria está activada, `ENGRAM_HTTP_TOKEN` está definido y la conversación tiene notas, la pregunta pasa a `Delete conversation? y · n · a also delete its N notes`: `y` borra solo la conversación, `a` borra la conversación y después sus notas (`DELETE /observations/{id}` con el token, una por una), y cualquier otra tecla cancela. Sin token o con la memoria desactivada la pregunta es la normal y, tras borrar, una línea indica que las notas siguen en engram. El resultado de `a` se muestra como `deleted N notes from engram`, `deleted K of N notes from engram` o `could not delete notes: <motivo>`; la conversación ya está borrada en cualquiera de los casos. Al superar el límite de 200 conversaciones, la más antigua se elimina **sin** tocar sus notas.

**Retomar una conversación:** al retomarla se reutiliza la sesión de engram que usaron sus notas (con su proyecto), así que las notas nuevas siguen en la misma sesión y se cierra una sola vez. Una conversación retomada que no escribe nada no cierra ninguna sesión. Sus notas nuevas siguen en el proyecto de engram con el que se guardó la conversación, aunque la retomes desde otro directorio.

Límites: la memoria se envía con la conversación a los adaptadores de Ollama, claude, codex y gemini (en los tres CLIs, **al proveedor**).

### Publicar una versión (mantenedores)

Las versiones se publican subiendo la etiqueta `vX.Y.Z`, una vez actualizados `version` en `Cargo.toml` y la sección correspondiente de `CHANGELOG.md`. La etiqueta debe apuntar a un commit que ya esté en `main`; si no, el workflow falla:

```bash
git tag vX.Y.Z && git push origin vX.Y.Z
```

El workflow `.github/workflows/release.yml` comprueba que la etiqueta coincida con `Cargo.toml`, ejecuta fmt, clippy y los tests, publica en crates.io y crea la GitHub Release con las notas de esa versión del `CHANGELOG.md`. Relanzarlo es seguro: si la versión ya está en crates.io, el paso de publicación se omite. Las notas de la release se regeneran siempre a partir de `CHANGELOG.md`, así que cualquier edición manual en GitHub se sobrescribe.

La publicación usa Trusted Publishing (OIDC), así que no hay ningún token guardado en el repositorio. Configuración única en crates.io: en la página del crate, **Settings → Trusted Publishing**, añade un repositorio de GitHub con `pichu2707/lazarobox-shell` como repositorio, `release.yml` como workflow y sin entorno (environment).

---

## 📁 Arquitectura del Workspace en Rust

```text
lazarobox-shell/
├── Cargo.toml
├── README.md
├── CHANGELOG.md
├── docs/                         # Recursos y checklists
├── tests/                        # Tests de integración (jail, shim MCP, CLIs)
└── src/
    ├── main.rs                   # Entrypoint y argumentos de la CLI
    ├── lib.rs
    ├── app.rs                    # Estado central de la aplicación
    ├── app/                      # Chat, historial, notas, etiquetas, tools y actualizaciones
    │
    ├── core/                     # NÚCLEO PURO (sin IO)
    │   ├── ai/                   # Dominio del chat de IA
    │   │   ├── chat/             # Motores (claude, codex, gemini), streams, transcript, tools
    │   │   ├── memory/           # Memoria del proyecto (engram)
    │   │   └── tools/            # Herramientas que los motores pueden pedir
    │   ├── config.rs             # Configuración global
    │   ├── layout.rs             # Identidad y geometría de panes
    │   ├── pane.rs               # Pane de terminal (emulador vt100)
    │   ├── pty/                  # Puerto PTY
    │   ├── menu.rs               # Menú
    │   └── update.rs             # Avisos de actualización
    │
    ├── runtime/                  # EFECTOS: IO, procesos y red
    │   ├── chat/                 # Adaptadores: CLIs externos y Ollama
    │   ├── chat_store/           # Persistencia del historial
    │   ├── engram/               # Cliente de memoria engram
    │   ├── mcp/                  # Servidores MCP propios y externos
    │   ├── tools/                # Ejecución de tools y aprobación
    │   ├── jail/                 # Confinamiento de los CLIs
    │   ├── loopback/             # Cliente HTTP local
    │   └── ai_detect.rs          # Detección de motores
    │
    └── ui/                       # CAPA DE INTERFAZ TUI (Ratatui)
        ├── components/
        │   ├── tab_bar.rs        # Barra superior de pestañas
        │   ├── statusline.rs     # Barra de estado inferior
        │   ├── chat.rs           # Panel de chat con streaming
        │   ├── terminal_view.rs  # Vista de los panes de terminal
        │   └── menu.rs           # Menú emergente
        └── theme.rs              # Paleta lazarobox
```
