# Conectar servicios MCP desde lazarobox: guía paso a paso

Esta guía te lleva, servicio por servicio, desde cero hasta una herramienta funcionando en el chat: **Clarity**, **Google Analytics 4 (GA4)**, **Search Console (GSC)**, **Google Workspace** (borradores de Gmail, Docs y Hojas de cálculo) y **Apps Script**. Se hace desde la pestaña **Connectors** del menú; lazarobox no incluye ni reimplementa ningún conector: ejecuta servidores MCP existentes que tú instalas y declaras.

Todo lo que ves aquí sale del código de la versión actual. Los textos entre comillas (avisos y notas del pie) son los que aparecen en pantalla, en inglés.

## Ruta rápida

1. Abre el menú con `Ctrl+Espacio` `m`, pulsa `4` (pestaña **Connectors**) y entra en el servicio con Enter.
2. Pulsa `i` para instalar el servidor (te enseña el comando y espera una `y`).
3. Pega en `config.toml` (`~/.config/lazarobox/config.toml`) la entrada que muestra la pantalla (`c` la copia).
4. Rellena los valores con `f` (y `o` para el cliente OAuth de GA4 y GSC).
5. Inicia sesión: `l` (GA4 y Apps Script) o `g` (GSC con OAuth y Google Workspace).
6. Comprueba con `p` que el servidor arranca (`mcp: started <nombre> (N tools)`).
7. Abre un chat con **Tools** activado y pide algo. Cada llamada a una herramienta de estos servidores espera tu `y`.

## Índice

- [Antes de empezar](#antes-de-empezar)
- [Dónde se guarda cada cosa](#dónde-se-guarda-cada-cosa)
- [Clarity](#clarity)
- [Crear tu propio cliente OAuth de Google](#crear-tu-propio-cliente-oauth-de-google)
- [Google Analytics 4](#google-analytics-4)
- [Search Console](#search-console)
- [Google Workspace: Gmail, Docs y Hojas](#google-workspace-gmail-docs-y-hojas)
- [Apps Script](#apps-script)
- [Riesgos de un vistazo](#riesgos-de-un-vistazo)
- [Cómo desconectar un servicio](#cómo-desconectar-un-servicio)
- [Apéndice: las teclas del área Connectors](#apéndice-las-teclas-del-área-connectors) (con el estado de cada servicio y cómo se rellena un valor)
- [Solución de problemas](#solución-de-problemas)

## Antes de empezar

| Qué necesitas | Para qué |
|---|---|
| Un chat con **Tools** activado | Los servidores solo arrancan cuando un turno con herramientas los necesita. |
| `npm` (Clarity, Apps Script), `pipx` (GA4) o `uv` (GSC, Workspace) | El comando de instalación de cada entrada. |
| `gcloud` (GA4) | El inicio de sesión de GA4 lo ejecuta `gcloud`. |
| Un proyecto propio en Google Cloud (GA4, GSC, Workspace) | Tú creas el cliente OAuth; lazarobox no usa uno compartido. |
| Un terminal con navegador accesible | El inicio de sesión de Google abre una vez el navegador. Si no se abre, el enlace sigue visible en pantalla. |

Dónde conseguir las herramientas: `uv` en <https://docs.astral.sh/uv/>, `pipx` en <https://pipx.pypa.io/>, Node.js y `npm` en <https://nodejs.org/> y `gcloud` en <https://cloud.google.com/sdk/docs/install>.

Dos reglas que valen para todos los servicios:

- **La entrada de `config.toml` la pegas tú.** lazarobox no escribe `config.toml` ni la sección `ai.mcp.*`. Pega la entrada, guarda el archivo y reinicia lazarobox para que los chats en marcha la carguen (el área lo recuerda con `restart lazarobox to use it in chats`).
- **Cada servidor corre como tú.** Un servidor MCP tuyo se ejecuta con tu usuario, en tu `$HOME`, y puede leer tus archivos, incluido `secrets.toml`. Declara solo servidores en los que confíes y mantén las versiones fijadas que el catálogo instala (por ejemplo `workspace-mcp==2.4.1`).

## Dónde se guarda cada cosa

| Qué | Dónde | Quién lo escribe |
|---|---|---|
| Entrada del servidor (`[[ai.mcp.servers]]`) | `~/.config/lazarobox/config.toml` | Tú, a mano. |
| Valores (tokens, ids, rutas) | `~/.config/lazarobox/secrets.toml` (o `$XDG_CONFIG_HOME/lazarobox/secrets.toml`), permisos `0600` | Tú a mano, o el guardado del menú (`f`). |
| Copia del archivo anterior | `secrets.toml.bak`, `0600`, junto al archivo | El guardado del menú, antes de reemplazar el archivo. Se queda hasta que la borres con `d`. |
| Cliente OAuth de escritorio | `~/.config/lazarobox/google-oauth-client.json`, `0600`; la copia anterior en `google-oauth-client.json.bak` | El formulario `o` (GA4 y GSC). |
| Tokens de Google | Los guarda cada servidor, **nunca lazarobox**: `~/.config/gcloud` (GA4), `~/.config/mcp-gsc/token.json` (GSC con OAuth), `~/.google_workspace_mcp/credentials` (Workspace, JSON en texto plano) y `~/.clasprc.json` (Apps Script) | El servidor o la herramienta de login. |

Qué hace lazarobox con esos archivos y qué no:

- El menú escribe **solo** un valor de `secrets.toml` por guardado (y el archivo de cliente OAuth y el borrado de `.bak`). No escribe `config.toml` ni `ai.mcp.*`, y ningún valor suyo se escribe en `config.toml`.
- Nunca guarda, lee ni copia un token de Google. Para saber si una cuenta está enlazada solo comprueba, con `stat`, si existe el archivo del token o si cambió.
- El guardado se niega a escribir (y lo dice en el pie) si la carpeta de configuración es un enlace simbólico, no es tuya o es escribible por el grupo o por otros, y si `secrets.toml` es un enlace simbólico, no es tuyo, tiene permisos más abiertos, es de solo lectura, tiene otros enlaces duros o no es TOML válido. En todos esos casos puedes seguir editándolo a mano.
- La lectura (el arranque de un servidor) se niega a usar un `secrets.toml` que no tenga modo `0600`, que no sea un archivo normal o que pase de 64 KiB, y en ese caso el servidor arranca solo con tu entorno.
- Las herramientas de lectura del chat rechazan `~/.config/lazarobox` entero, las carpetas de tokens de los conectores (`~/.config/gcloud`, `~/.config/mcp-gsc`, `~/Library/Application Support/mcp-gsc` (macOS también acepta esa carpeta para `GSC_CONFIG_DIR`), `~/.google_workspace_mcp`, `~/.clasprc.json`) y `~/.config/gh`. El modelo no lee tus valores ni tus tokens por esa vía. Un servidor MCP sí los puede leer, porque corre como tú. Esta lista es de mejor esfuerzo y solo cubre las rutas predeterminadas: si usas un `$XDG_CONFIG_HOME` propio, esa carpeta no queda protegida.

## Clarity

**Qué da.** Tres herramientas de solo lectura: consultar el panel de analítica, listar grabaciones de sesión y buscar en la documentación.

**Qué cuesta.** La API de Clarity permite 10 peticiones al día por proyecto, solo los últimos 1 a 3 días y hasta 1000 filas por respuesta, sin paginación. No usa Google.

**Paso a paso.**

1. En los ajustes de tu proyecto de Clarity abre *Data Export*, genera un token de API con permisos de administrador y cópialo.
2. Entra en **Clarity** en el área y pulsa `i` (`npm i -g @microsoft/clarity-mcp-server`).
3. Pega en `config.toml` (`c` copia la entrada):

   ```toml
   [[ai.mcp.servers]]
   name = "clarity"
   command = "clarity-mcp-server"
   env = ["CLARITY_API_TOKEN"]
   ```

4. Pulsa `f`, Enter sobre `CLARITY_API_TOKEN`, pega el token y confirma. Queda en `secrets.toml`, tabla `[clarity]`.
5. Pulsa `p` y confirma con `y`: debes ver `mcp: started clarity (3 tools)`.

## Crear tu propio cliente OAuth de Google

GA4, GSC (vía OAuth) y Google Workspace usan **tu** cliente OAuth, como harías con un n8n autoalojado: ningún cliente compartido pasa por tu cuenta. Se crea una vez y sirve para todos.

1. En la [consola de Google Cloud](https://console.cloud.google.com/), crea o elige un proyecto tuyo.
2. En *APIs y servicios → Biblioteca*, activa las APIs que vayas a usar: *Google Analytics Admin API* y *Google Analytics Data API* (GA4), *Search Console API* (GSC), *Gmail API*, *Google Docs API*, *Google Sheets API* y *Google Drive API* (Workspace).
3. En *APIs y servicios → Pantalla de consentimiento de OAuth* (el nombre del menú puede variar: *Google Auth Platform*), configura la pantalla (tipo *Externo* si usas una cuenta personal) y añade tu correo como usuario de prueba.
4. En *APIs y servicios → Credenciales → Crear credenciales → ID de cliente de OAuth*, elige el tipo **Aplicación de escritorio** (*Desktop app*). Descarga el JSON o copia el id y el secreto.
5. Después de añadir los usuarios de prueba, **publica la pantalla de consentimiento**. Un cliente que sigue en estado *Testing* recibe tokens de refresco que caducan a los 7 días, y tendrías que volver a iniciar sesión cada semana.

Al iniciar sesión verás un aviso de aplicación no verificada: *Avanzado → Ir a … (no seguro)*; es tu propio cliente.

Qué haces con el cliente según el servicio:

| Servicio | Dónde va el cliente |
|---|---|
| GA4 y GSC (OAuth) | Formulario `o` (id y secreto), que escribe `~/.config/lazarobox/google-oauth-client.json`; o guarda tú el JSON descargado con ese nombre. |
| Google Workspace | `f`: `GOOGLE_OAUTH_CLIENT_ID` (visible) y `GOOGLE_OAUTH_CLIENT_SECRET` (enmascarado) en `secrets.toml`, tabla `[google]`. |

### El formulario `o` (archivo de cliente)

Se abre desde la lista de valores (`f`) de GA4 o de GSC.

1. Pulsa `o`. Escribe el **id del cliente** (debe terminar en `.apps.googleusercontent.com`, solo caracteres ASCII imprimibles) y Enter.
2. Escribe el **secreto del cliente** (una línea, sin espacios, enmascarado) y Enter.
3. Si el archivo ya existe, pregunta `replace google-oauth-client.json? (y/n)` antes de reemplazarlo; la versión anterior se queda en `google-oauth-client.json.bak`.

Avisos que puedes ver: `the client id must end in .apps.googleusercontent.com and use printable ASCII`, `the client secret must be one line of printable ASCII, without spaces`, `nothing typed; Esc cancels` y `google-oauth-client.json exists; open the form again to replace it`.

## Google Analytics 4

**Qué da.** Diez herramientas de solo lectura de la Admin API y la Data API (resúmenes de cuenta, detalle de propiedad, informes, informes en tiempo real, embudos, eventos clave…). Las herramientas del servidor solo leen GA4, pero el inicio de sesión también pide el permiso `cloud-platform` (acceso amplio a las APIs de Google Cloud de tu cuenta) y las credenciales quedan en `~/.config/gcloud`, legibles por cualquier programa con tu usuario.

**Qué cuesta.** Necesitas un cliente OAuth propio ([cómo crearlo](#crear-tu-propio-cliente-oauth-de-google)) y un proyecto de Google Cloud con las dos APIs activas. Las credenciales las guarda `gcloud` en `~/.config/gcloud` (las credenciales predeterminadas de la aplicación).

**Paso a paso.**

1. Crea el cliente OAuth de escritorio y activa *Google Analytics Admin API* y *Google Analytics Data API*.
2. Instala el servidor con `i` (`pipx install analytics-mcp`) y la CLI `gcloud` si no la tienes.
3. Pega en `config.toml`:

   ```toml
   [[ai.mcp.servers]]
   name = "ga4"
   command = "analytics-mcp"
   env = ["GOOGLE_CLOUD_PROJECT"]
   ```

4. Pulsa `f`: Enter sobre `GOOGLE_CLOUD_PROJECT` y escribe el id de tu proyecto (es visible, no confidencial). Pulsa `o` y guarda el id y el secreto del cliente.
5. Vuelve a la pantalla del servicio y pulsa `l`. El comando que se escribe en una pestaña nueva es `gcloud auth application-default login --scopes https://www.googleapis.com/auth/analytics.readonly,https://www.googleapis.com/auth/cloud-platform --client-id-file=~/.config/lazarobox/google-oauth-client.json`. Confirma con `y`, ve la pestaña y completa el inicio de sesión en el navegador. Si el navegador no se abre, copia el enlace que `gcloud` imprime en esa pestaña.
6. Pulsa `p`: `mcp: started ga4 (10 tools)`.

## Search Console

**Qué da.** Consultas de analítica de búsqueda, inspección de URL, sitemaps y listado de propiedades. Servidor: `mcp-search-console` (versión fijada a `0.4.2`). Hay **dos vías** y eliges una con `w`.

**Qué cuesta.**

- Declara el servidor con `name = "gsc"` exactamente: el filtro de la herramienta `reauthenticate` (oculta, porque borraría el token e iniciaría un flujo de navegador) y la comprobación de arranque solo se aplican a ese nombre. Con otro nombre no te cubren.
- `GSC_ALLOW_DESTRUCTIVE` no está declarada, así que el servidor rechaza añadir o borrar propiedades y borrar sitemaps. Enviar sitemaps (`manage_sitemaps`, `submit_sitemap`) cuenta como escritura y pide tu `y`, como toda llamada.
- Lo que consulta la herramienta (propiedad, fechas, consultas, URL inspeccionadas) se envía a Google desde tu máquina, y los resultados llegan al modelo.

La entrada de `config.toml` es la misma para las dos vías (las dos listas de nombres van juntas, así que cambiar de vía no toca `config.toml`):

```toml
[[ai.mcp.servers]]
name = "gsc"
command = "mcp-search-console"
env = ["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_CONFIG_DIR", "GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]
```

### Vía 1: OAuth con Link with Google (la predeterminada)

1. Crea el cliente OAuth de escritorio y activa la *Search Console API* ([cómo crearlo](#crear-tu-propio-cliente-oauth-de-google)). El cliente de GA4 sirve.
2. Instala con `i` (`uv tool install mcp-search-console==0.4.2`) y pega la entrada.
3. Pulsa `f`, luego `o`, y guarda el id y el secreto del cliente (o guarda tú el JSON en `~/.config/lazarobox/google-oauth-client.json`).
4. En la lista de valores, Enter sobre cada fila de OAuth: `GSC_OAUTH_CLIENT_SECRETS_FILE` (la ruta del cliente), `GSC_CONFIG_DIR` (`~/.config/mcp-gsc`, ruta absoluta) y `GSC_SKIP_OAUTH` con `"false"`. Las tres traen un valor sugerido y se guardan con Enter. `GSC_CONFIG_DIR` debe quedar bajo `~/.config/mcp-gsc`: si no, el arranque se rechaza (`GSC_CONFIG_DIR must be under ~/.config/mcp-gsc; fix it in Connectors (f)`).
5. Vuelve a la pantalla del servicio y pulsa `g`. Primero comprueba `uv` y que el cliente exista (`checking the client file and uv…`), y luego te enseña la línea exacta que va a escribir (`link gsc with Google? (y/n)`). Con `y`, esa línea se escribe en una pestaña nueva, abre el navegador una vez y, al terminar, imprime `Search Console linked`. La línea crea la carpeta del token y el token privados (umask `077`).
6. La fila pasa a `declared · linked`. Pulsa `p`: `mcp: started gsc (N tools)`.

**Enlaza antes de usar el chat.** Un arranque en el chat sin token se rechaza con `mcp: gsc unavailable: not linked; open Connectors and use Link with Google`; sin los valores de OAuth, con `mcp: gsc unavailable: fill the OAuth values in Connectors (f)`. Es a propósito: sin token, el servidor intentaría iniciar un flujo de navegador dentro de una llamada y el chat podría quedarse esperando hasta 15 minutos. Si un token caducado deja una llamada colgada, pulsa Esc para cancelarla y vuelve a enlazar con `g`.

### Vía 2: cuenta de servicio

1. En tu proyecto de Google Cloud activa la *Search Console API* y crea una cuenta de servicio (*IAM y administración → Cuentas de servicio*). Crea una clave JSON y guárdala como `~/.config/lazarobox/gsc-service-account.json` con permisos `0600` (`chmod 600`).
2. En Search Console, *Configuración → Usuarios y permisos*, añade el correo de la cuenta de servicio como usuario de la propiedad. *Restricted* basta para la analítica de búsqueda; si la inspección de URL se deniega, usa *Full*.
3. Pulsa `w` para elegir **Service account**. En `f`, Enter sobre `GSC_CREDENTIALS_PATH` y escribe la ruta del JSON (es una ruta: el contenido de la clave nunca pasa por lazarobox), y Enter sobre `GSC_SKIP_OAUTH` con `"true"`.
4. No hay paso de inicio de sesión (`g` solo funciona en la vía OAuth: en la otra dice `linking needs the OAuth way (w chooses it)`). Pulsa `p`.

## Google Workspace: Gmail, Docs y Hojas

**Qué da.** El servidor `workspace-mcp` (versión fijada a `2.4.1`), con 41 herramientas visibles para el modelo según el catálogo actual:

| Producto | Qué puede hacer |
|---|---|
| Gmail | Buscar y leer mensajes, hilos y etiquetas, leer adjuntos y **crear borradores**. Nada más: son 8 herramientas en una lista de permitidas. |
| Docs | Leer, crear y modificar documentos. |
| Hojas de cálculo | Leer, crear y modificar hojas. |

**Gmail solo crea borradores.** lazarobox registra únicamente las 8 herramientas de Gmail de esta lista: `search_gmail_messages`, `get_gmail_message_content`, `get_gmail_messages_content_batch`, `get_gmail_attachment_content`, `draft_gmail_message`, `get_gmail_thread_content`, `get_gmail_threads_content_batch` y `list_gmail_labels`. `send_gmail_message` no está en la lista, y además el servidor se arranca con `--disabled-tools send_gmail_message`. Tú envías el borrador desde Gmail. `start_google_auth` también está oculta del modelo: solo la usa el botón Link.

**Qué cuesta (riesgos que debes conocer).**

- **El token de Gmail podría enviar correo a nivel de API.** El servidor guarda un token con el permiso `gmail.compose`, que permite enviar. lazarobox no registra ninguna herramienta de envío, pero el token puede hacerlo si otro programa lo usa. Es el riesgo residual aceptado.
- **Tokens en texto plano.** `workspace-mcp` guarda sus tokens como JSON legible en `~/.google_workspace_mcp/credentials`. lazarobox se niega a leer esa carpeta, pero otros programas con tu usuario sí pueden.
- **Los permisos de Docs y Hojas incluyen `drive.readonly`**: el servidor puede leer todo tu Drive, no solo los documentos que menciones.
- **Archivos locales.** `get_gmail_attachment_content` guarda adjuntos en `~/.workspace-mcp/attachments`, fuera de las comprobaciones de escritura de lazarobox: cada llamada pide tu `y`. Ojo: `~/.workspace-mcp/attachments` **no** está protegida contra lectura, así que las herramientas de lectura del chat pueden leer los adjuntos guardados ahí. `WORKSPACE_MCP_DISABLE_LOCAL_FILES` viene en `"true"` y hace que `draft_gmail_message` no pueda adjuntar archivos locales por ruta (comprobado en la versión 2.4.1; el resto de herramientas siguen funcionando). Ponla en `"false"` si quieres permitirlo.
- **`name = "google"` es obligatorio.** La lista de Gmail y las herramientas ocultas se aplican por el nombre `google`. Con otro nombre se registran todas las herramientas de Gmail, incluida la de envío.
- No declares `WORKSPACE_MCP_CREDENTIALS_DIR` ni `GOOGLE_MCP_CREDENTIALS_DIR`: el arranque se rechaza (`WORKSPACE_MCP_CREDENTIALS_DIR / GOOGLE_MCP_CREDENTIALS_DIR are not supported; remove them from the entry`), porque el servidor guardaría sus tokens fuera de la carpeta que las herramientas de lectura protegen.

**Paso a paso.**

1. Crea el cliente OAuth de escritorio y activa Gmail, Docs, Sheets y Drive ([cómo crearlo](#crear-tu-propio-cliente-oauth-de-google)).
2. Instala con `i` (`uv tool install workspace-mcp==2.4.1`).
3. Pega en `config.toml`:

   ```toml
   [[ai.mcp.servers]]
   name = "google"
   command = "workspace-mcp"
   args = ["--single-user", "--permissions", "gmail:readonly+drafts", "docs:full", "sheets:full", "--disabled-tools", "send_gmail_message"]
   env = ["GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET", "USER_GOOGLE_EMAIL", "WORKSPACE_MCP_DISABLE_LOCAL_FILES"]
   ```

4. Pulsa `f` y rellena: `GOOGLE_OAUTH_CLIENT_ID` (visible), `GOOGLE_OAUTH_CLIENT_SECRET` (enmascarado), `USER_GOOGLE_EMAIL` (tu correo de Google) y `WORKSPACE_MCP_DISABLE_LOCAL_FILES` (valor sugerido `"true"`).
5. Vuelve a la pantalla del servicio y pulsa `g`. Te pregunta `start workspace-mcp and ask Google to link <tu correo>? (y/n)` y muestra el comando que arranca, sin shell. Con `y`, lazarobox arranca el servidor como lo haría un chat, le pide su enlace de inicio de sesión y abre ese enlace en el navegador (o lo copia y lo muestra: `Open this link if the browser did not:`). Mientras tanto el pie dice `starting workspace-mcp… · Esc cancels` y luego `waiting for Google… · Esc cancels`.
6. Inicia sesión en Google y acepta los permisos. El servidor recibe la respuesta en un puerto local (8000 a 8004) y guarda el token; la fila pasa a `declared · linked` y el pie dice `linked google`. El servidor se detiene siempre al terminar.
7. Pulsa `p`: `mcp: started google (41 tools)` (el número cambia si una versión nueva del servidor publica otras herramientas).

Si el servidor abre además su propia pestaña, usa cualquiera de las dos; solo una completa el inicio de sesión. Mientras espera, Link abre una escucha local en los puertos 8000 a 8004.

lazarobox solo acepta un enlace de `accounts.google.com` que lleve **tu** id de cliente y vuelva a un puerto local 8000 a 8004; cualquier otro se rechaza con un aviso, sin abrirlo.

## Apps Script

**Qué da.** El servidor MCP de `clasp` (versión fijada a `3.4.1`; se necesita la 3.4.0 o posterior): listar proyectos, crear proyectos, subir código (`push_files`), traer código (`pull_files`) y clonar (`clone_project`). No hay valores que rellenar ni cliente que crear: `clasp` usa su propio cliente OAuth.

**Qué cuesta.** Cuatro de las cinco herramientas escriben: `push_files` sube código desde un directorio de tu disco y ese código se ejecuta con los permisos de tu script; `pull_files` y `clone_project` escriben archivos locales y pueden sobrescribirlos; `create_project` crea un proyecto en tu cuenta. Cada llamada pide tu `y`: lee el aviso (servidor, herramienta y argumentos) antes de aprobar, sobre todo con `push_files`. El token queda en `~/.clasprc.json` (lazarobox se niega a leerlo).

**Paso a paso.**

1. Activa la API de Apps Script para tu cuenta en <https://script.google.com/home/usersettings>.
2. Instala con `i` (`npm i -g @google/clasp@3.4.1`).
3. Pega en `config.toml`:

   ```toml
   [[ai.mcp.servers]]
   name = "apps-script"
   command = "clasp"
   args = ["mcp"]
   ```

4. Pulsa `l` (`clasp login`), confirma con `y` y completa el navegador. No hay `env`, así que no hay `f` ni tabla en `secrets.toml`.
5. Pulsa `p`: `mcp: started apps-script (5 tools)`.

## Riesgos de un vistazo

| Servicio | Lo que debes saber |
|---|---|
| Clarity | Solo lectura. Token de administrador del proyecto en `secrets.toml`. 10 peticiones al día. |
| GA4 | Herramientas de solo lectura, pero el token incluye `cloud-platform` (acceso amplio a Google Cloud). Las credenciales las guarda `gcloud` y las puede leer cualquier programa con tu usuario. |
| GSC | Con `name = "gsc"`. Las herramientas de escritura (sitemaps) piden `y`; añadir y borrar propiedades está desactivado. Un token caducado puede colgar una llamada hasta Esc: enlaza de nuevo. |
| Google Workspace | Con `name = "google"`. Lista de 8 herramientas de Gmail, solo borradores. `gmail.compose` puede enviar a nivel de API. `drive.readonly` lee todo el Drive. Tokens en texto plano. |
| Apps Script | Cuatro herramientas de escritura. `push_files` ejecuta código con los permisos del script. |
| Todos | Cada llamada pide `y`, también en modo `auto`. Los resultados llegan al modelo como datos no fiables. |

## Cómo desconectar un servicio

Para dejar de usar un servicio sin borrar nada: ponle `enabled = false` en `config.toml` o borra su entrada, y reinicia lazarobox.

Para borrar también lo que guardaste:

1. Borra la tabla del servicio en `secrets.toml` (a mano: el menú no borra valores) y, con `d` en la lista de valores, la copia `secrets.toml.bak`, que conserva tus valores anteriores. Si guardaste el cliente OAuth, borra también `~/.config/lazarobox/google-oauth-client.json` y su `.bak`.
2. Revoca el acceso en tu cuenta de Google: <https://myaccount.google.com/permissions>.
3. Borra el token según el servicio:

| Servicio | Qué borrar |
|---|---|
| Clarity | Sustituye o elimina el token en *Settings → Data Export* (la documentación de Clarity no describe una revocación explícita). |
| GA4 | `gcloud auth application-default revoke` (no toca credenciales que vengan de una variable de entorno o de una cuenta de servicio). |
| GSC (OAuth) | La carpeta `~/.config/mcp-gsc` (`token.json`). |
| GSC (cuenta de servicio) | Quita el correo de la cuenta de servicio en *Usuarios y permisos* de la propiedad, y borra la clave en *IAM y administración → Cuentas de servicio → Claves* y el archivo `gsc-service-account.json`. |
| Google Workspace | La carpeta `~/.google_workspace_mcp/credentials` y, si guardaste adjuntos, `~/.workspace-mcp/attachments`. |
| Apps Script | `~/.clasprc.json` (o `clasp logout`). |

## Apéndice: las teclas del área Connectors

El área tiene tres pantallas: la lista de servicios, la pantalla de un servicio y la lista de valores (`f`).

| Pantalla | Tecla | Qué hace |
|---|---|---|
| Lista | `j` `k`, Enter, Esc | Moverte, abrir un servicio, volver. |
| Servicio | `j` `k` | Desplazar la guía si no cabe. |
| Servicio | `i` | Instalar: muestra el comando y espera `y`. |
| Servicio | `l` | Iniciar sesión (solo GA4 y Apps Script). |
| Servicio | `g` | **Link with Google** (solo GSC con OAuth y Google Workspace). |
| Servicio | `p` | Probar: arranca el servidor, pide su lista de herramientas y lo detiene. |
| Servicio | `f` | Abrir la lista de valores para rellenarlos. |
| Servicio | `w` | Elegir la vía (solo GSC: OAuth o cuenta de servicio). También funciona dentro de `f`. |
| Servicio | `c` | Copiar la entrada de `config.toml`. |
| Servicio | `t` | Copiar la tabla de `secrets.toml` (vacía). |
| Servicio | `1` a `9` | Abrir un enlace de la guía (o copiarlo si no hay navegador). El último enlace es esta guía. |
| Valores (`f`) | `j` `k`, Enter | Moverte y editar el valor de la fila. |
| Valores (`f`) | `o` | Formulario del archivo de cliente OAuth (solo GA4 y GSC). |
| Valores (`f`) | `w` | Cambiar de vía (GSC). |
| Valores (`f`) | `d` | Borrar la copia `secrets.toml.bak` (solo si existe). |
| Todas | Esc | Volver un nivel o cancelar. |

Dos detalles de las confirmaciones: solo una `y` suelta confirma (`Ctrl+y` o `Alt+y` no) y cualquier otra tecla cancela (`cancelled`). Si el terminal es tan estrecho que el comando no cabe entero, lazarobox no lo ofrece y avisa con `widen the terminal to read the command`: lo que ves es siempre lo que se ejecuta.

### El estado de cada servicio en la lista

| Estado | Significado |
|---|---|
| `not declared` | No hay un servidor con ese `name` en `config.toml`. |
| `disabled` | La entrada tiene `enabled = false`. |
| `values missing: <NOMBRES>` | Falta algún valor en `secrets.toml`. |
| `declared` | Declarado y con sus valores. |
| `declared · linked` / `declared · not linked` | Solo GSC con OAuth y Google Workspace: si existe su archivo de token (se comprueba con `stat`, nunca se abre). |
| `· probe: started`, `failed` o `stopped` | Resultado de la última prueba de esta sesión. |

### Cómo se rellena un valor (`f`)

- Una fila con valor confidencial muestra solo `set` o `not set`; al escribirlo se ve una máscara fija de ocho puntos (`••••••••`), sin revelar la longitud. Los valores que no son confidenciales (por ejemplo `GOOGLE_CLOUD_PROJECT` o `USER_GOOGLE_EMAIL`) se muestran enteros.
- Una fila con valor sugerido muestra `not set · Enter writes <valor>`: Enter lo guarda sin teclear nada.
- Enter en una fila sin valor sugerido abre el campo: escribe o pega y confirma con Enter; Esc descarta el texto. El valor debe ser una sola línea, sin caracteres de control y de hasta 4096 bytes.
- Si ya hay un valor guardado, antes de reemplazarlo pregunta `replace the saved <NOMBRE>? (y/n)`. Si el comando declarado en `config.toml` no es el del catálogo, pregunta antes `save <NOMBRE> for a server that runs <comando>? (y/n)`.
- Enter sobre un campo vacío no guarda nada (`empty value not saved; edit secrets.toml by hand to remove a value`): para quitar un valor, edita `secrets.toml` a mano.
- Si guardas, el pie dice `saved <NOMBRE>` y, cuando había un archivo previo, `saved <NOMBRE>; the previous values stay in secrets.toml.bak (0600) until you delete it`. La fila `restart the chat or probe again to use it` te recuerda que el cambio vale en el siguiente arranque del servidor, no antes.

## Solución de problemas

Los textos son los que ves en el pie del menú o en el chat. Ninguno contiene un valor guardado: como mucho, el nombre de una tabla o de una clave.

### Antes de arrancar

| Aviso | Causa y arreglo |
|---|---|
| `not declared: paste the entry into config.toml first` | No hay un servidor con ese `name`. Pega la entrada y reabre el área. |
| `disabled in config.toml` | La entrada tiene `enabled = false`. |
| `restart lazarobox to use it in chats` | Cambiaste `config.toml` y los chats en marcha cargaron la lista anterior. Reinicia lazarobox. |
| `values missing: <NOMBRES>` | Rellénalos con `f`. |
| `a probe is already running` | Hay una prueba o un enlace en curso; espera o pulsa Esc. |
| `this entry has no login` / `this entry has one way` / `this entry has no values to fill` | Esa tecla no existe para ese servicio. |
| `no such key here` | Tecla que esa pantalla no usa. |

### Al guardar valores

| Aviso | Causa y arreglo |
|---|---|
| `the value must be one line without control characters` | Un salto de línea o un carácter de control (también al pegar). Pégalo sin ellos. |
| `the value is longer than 4096 bytes` | Demasiado largo para un campo. |
| `empty value not saved; edit secrets.toml by hand to remove a value` | El menú no borra valores. |
| `HOME is not an absolute path; edit secrets.toml by hand` | `$HOME` no es una ruta absoluta; no se puede calcular el valor sugerido. |
| `the config folder is a symlink; edit secrets.toml by hand` | Solo se bloquea el guardado del menú; leer y editar a mano sigue funcionando. |
| `the config folder is not owned by you; edit secrets.toml by hand` | La carpeta no es tuya. |
| `the config folder is writable by group or others; run chmod 700 <carpeta>` | Con umask `002` la carpeta queda en `0775`. Arreglo: `chmod 700 ~/.config/lazarobox`. |
| `secrets.toml is a symlink; edit it by hand` / `secrets.toml is not owned by you; edit it by hand` / `secrets.toml has other hard links; edit it by hand` | El archivo no es un archivo normal tuyo. |
| `secrets.toml is read-only (mode 0400); chmod 600 <ruta> to save from the menu` | Dale `0600`. |
| `secrets.toml: not valid TOML (line N); fix it by hand, nothing was saved` | Corrige la línea indicada. El menú nunca sobrescribe un archivo que no entiende. |
| `secrets.toml: [<servidor>] is not a plain table; edit it by hand` / `secrets.toml: <servidor>.<NOMBRE>: expected a string; edit it by hand` | La forma del archivo no es la que el guardado sabe editar. |
| `secrets.toml changed on disk; nothing was saved, try again` | Otro programa lo cambió mientras guardabas. Repite. |
| `secrets.toml.bak is not a regular file of yours; left as it is` | La copia no es un archivo normal tuyo; se deja intacta. |
| `secrets.toml.bak: there is no backup` | `d` sin copia que borrar. |
| `secrets.toml: <servidor>.<NOMBRE> is not a value of this connector` | El nombre no pertenece a ese servicio. |

### Al leer `secrets.toml` en el chat

| Aviso | Causa y arreglo |
|---|---|
| `mcp: secrets.toml: mode must be 0600 (chmod 600 <ruta>)` | Permisos demasiado abiertos. El servidor arranca solo con tu entorno. |
| `mcp: secrets.toml: not a regular file` / `larger than 64 KiB` / `cannot be read` | El archivo no es utilizable. |
| `mcp: secrets.toml: not valid TOML (line N)` | Error de sintaxis; no se usa ningún valor. |
| `mcp: secrets.toml: <tabla>.<CLAVE>: expected a string` | El valor no es texto; esa clave se salta. |

### GSC (OAuth)

| Aviso | Causa y arreglo |
|---|---|
| `linking needs the OAuth way (w chooses it)` | `g` solo funciona en la vía OAuth. |
| `add your Desktop OAuth client first (o)` | Falta `google-oauth-client.json`. |
| `fill the OAuth values first (f)` / `fill the OAuth values in Connectors (f)` | Falta `GSC_OAUTH_CLIENT_SECRETS_FILE`, `GSC_CONFIG_DIR` o `GSC_SKIP_OAUTH` en `"false"`. |
| `the declared gsc entry does not pass GSC_CONFIG_DIR; paste the current entry` (o `GSC_OAUTH_CLIENT_SECRETS_FILE`) | Tu entrada es anterior: vuelve a copiarla con `c`. |
| `GSC_CONFIG_DIR must be under ~/.config/mcp-gsc; fix it in Connectors (f)` | La carpeta del token debe estar bajo `~/.config/mcp-gsc`. |
| `uv not found: install uv first` | Instala `uv`. |
| `mcp-search-console not installed; run: uv tool install --force mcp-search-console==0.4.2` (o `mcp-search-console <versión>; run: …`) | Falta el servidor o su versión no es la fijada. |
| `uv did not answer in 5 s` / `uv tool dir did not print one absolute folder` | Comprueba `uv` desde un terminal. |
| `a path holds a character the command cannot carry; choose another (f)` | La ruta lleva un carácter que la línea de enlace no puede escribir con seguridad. |
| `mcp: gsc unavailable: not linked; open Connectors and use Link with Google` | Falta el token: enlaza con `g` antes de usar el chat. |

### Google Workspace

| Aviso | Causa y arreglo |
|---|---|
| `fill GOOGLE_OAUTH_CLIENT_ID, … first (f)` | Faltan valores; el aviso nombra cuáles. |
| `the declared google entry does not pass <NOMBRE>; paste the current entry` | Entrada antigua: vuelve a copiarla con `c`. |
| `USER_GOOGLE_EMAIL is not a plain e-mail address; fix it in Connectors (f)` | Debe ser una dirección sin espacios ni caracteres raros. |
| `WORKSPACE_MCP_CREDENTIALS_DIR / GOOGLE_MCP_CREDENTIALS_DIR are not supported; remove them from the entry` | Quita esas dos variables de `env`. |
| `link failed: no sign-in link in the answer` | El servidor no devolvió un enlace; mira si añade un motivo (`link failed: <motivo>`). |
| `link failed: the answer holds a link that is not a Google sign-in link`, `link failed: the sign-in link is for another client id`, `link failed: the sign-in link does not return to a local port 8000-8004` o `link failed: the sign-in link repeats a parameter` | lazarobox rechazó el enlace por seguridad y no lo abrió. Revisa `GOOGLE_OAUTH_CLIENT_ID`. |
| `link failed: start_google_auth did not answer in 30 s` | El servidor no respondió; prueba `p` y revisa la instalación. |
| `link failed: the server stopped` | El servidor terminó antes de recibir tu respuesta. |
| `link cancelled` | Pulsaste Esc. |
| `link timed out after 5 min` | No completaste el navegador a tiempo. Pulsa `g` de nuevo. |

### Prueba (`p`)

| Aviso | Causa y arreglo |
|---|---|
| `mcp: <nombre> unavailable: timed out after N s` | El servidor no contestó a tiempo (la prueba termina sola a los 20 s). Comprueba la instalación con `i` y el comando declarado. |
| `mcp: <nombre> unavailable: <motivo>` | El servidor no pudo arrancar; el motivo es una línea sanitizada y nunca el stderr en bruto. |
| `mcp: <nombre> probe stopped` | Pulsaste Esc durante la prueba. |
| `<nombre>: <N> tools not in the catalog (class write)` | El servidor publica herramientas que el catálogo no conoce: se tratan como escritura. Todas piden `y` de todos modos. |

## Y ahora, ¿qué?

Abre un chat con **Tools** activado y pide algo sencillo (por ejemplo, «lista mis propiedades de Search Console»). Cada llamada te mostrará servidor, herramienta y argumentos, y esperará tu `y`. Para el modelo de amenazas completo de los servidores MCP propios, consulta la sección [Tus propios servidores MCP](../README.md#tus-propios-servidores-mcp-aimcpservers) del README.
