# Manual de EngineMD

Servidor de Markdown a HTML. Convierte archivos `.md` en páginas HTML con
temas, resaltado de código, matemáticas, diagramas, gráficos y soporte de
bóvedas de Obsidian.

---

## Índice

1. [Instalación](#instalación)
2. [Uso rápido](#uso-rápido)
3. [Comandos](#comandos)
4. [Flags del servidor](#flags-del-servidor)
5. [Páginas y frontmatter](#páginas-y-frontmatter)
6. [Assets JS/CSS (self-hosted)](#assets-jscss-self-hosted)
7. [Librerías y Chart.js](#librerías-y-chartjs)
8. [Soporte Obsidian](#soporte-obsidian)
9. [Extras de render](#extras-de-render)
10. [Modo daemon](#modo-daemon)
11. [Export estático](#export-estático)
12. [Temas CSS](#temas-css)
13. [Configuración global](#configuración-global)
14. [Docker y CI](#docker-y-ci)
15. [Limitaciones](#limitaciones)

---

## Instalación

```bash
git clone <repo>
cd enginemd
cargo build --release
./target/release/enginemd fetch   # descarga las librerías JS/CSS al caché local
```

## Uso rápido

```bash
# Crear un sitio nuevo en ~/.enginemd/sites/<name>/
enginemd new mi-docs

# Servidor de producción (http://0.0.0.0:10300)
enginemd

# Servidor de desarrollo con hot-reload (http://0.0.0.0:9696)
enginemd --watch
```

## Comandos

| Comando | Descripción |
|---------|-------------|
| `enginemd new <name>` | Crea un sitio nuevo en `~/.enginemd/sites/<name>/` |
| `enginemd up <path>` | Registra un directorio existente como sitio |
| `enginemd down <name>` | Elimina un sitio del listado |
| `enginemd fetch [--force]` | Descarga las librerías JS/CSS al caché local |
| `enginemd build --out DIR` | Exporta los sitios a HTML estático |
| `enginemd` | Inicia el servidor en `0.0.0.0:10300` |
| `enginemd --watch` | Servidor dev con hot-reload (`0.0.0.0:9696`) |
| `enginemd daemon start` | Inicia el servidor en segundo plano (persiste al reiniciar) |
| `enginemd daemon stop` | Detiene el daemon y desactiva el autoarranque |
| `enginemd daemon restart` | Reinicia el daemon |
| `enginemd daemon status` | Estado del proceso y del autoarranque |
| `enginemd daemon logs` | Últimas líneas del log del daemon |

## Flags del servidor

```bash
enginemd --path /ruta               # sirve un solo directorio (sin listado)
enginemd --port 8080                # puerto personalizado
enginemd --lang es                  # idioma
enginemd --js-support mathjax,mermaid,chartjs  # allowlist de librerías
enginemd --css-support dark         # tema CSS o ruta a un CSS propio
```

Los flags son globales: también valen tras un subcomando, p. ej.
`enginemd daemon start --port 8080`.

## Páginas y frontmatter

- `index.md` o `init.md` → página principal del sitio.
- `about.md` → `/about`.
- `docs/guide.md` → `/docs/guide`.
- Los archivos que no son `.md` (imágenes, PDF, HTML...) se sirven tal cual.

Si el archivo no define `title` en el frontmatter, se usa el primer `# Heading`.

Frontmatter YAML soportado:

```yaml
---
title: Mi página
description: Descripción corta
lang: es
tags: [rust, docs]      # string o lista
aliases: [otro-nombre]
cssclasses: wide-page   # clases para el <body>
draft: false
---
```

## Assets JS/CSS (self-hosted)

Todas las librerías se **descargan y se sirven localmente**; la página nunca
carga desde un CDN. Al arrancar, `auto_fetch` descarga lo que falte en segundo
plano, y también se descarga bajo demanda si falta.

- Caché en `~/.enginemd/js` y `~/.enginemd/css`; manifest de hashes SRI en
  `~/.enginemd/assets.json`.
- **Detección automática**: cada página carga solo lo que usa (bloques
  `mermaid`/`chart`, fórmulas `$...$`, etc.), aunque no esté en `--js-support`.
- `--js-support` es una **allowlist** de claves del catálogo.
- **SRI** activo por defecto (`sri: false` para desactivarlo) y `defer`.
- **Cache-Control**: assets con `?v=<hash>` e `immutable`; temas con `no-cache`.
- Si un asset no está y no se puede descargar, se avisa y se omite.
- `enginemd fetch --force` fuerza la re-descarga.
- `assets_dir` cambia la carpeta del caché; `cdn_base`/`cdn_fallbacks` cambian
  la fuente de descarga (compatible con jsdelivr/unpkg).

Catálogo actual: `mathjax`, `katex`, `mermaid`, `chartjs`, `highlight`,
`anchor`, `fontawesome`. El resaltado de código es server-side (syntect) por
defecto.

## Librerías y Chart.js

| Clave | Librería |
|-------|----------|
| `mathjax` | MathJax 3 — fórmulas LaTeX |
| `katex` | KaTeX — alternativa más ligera (excluyente con MathJax) |
| `mermaid` | Mermaid — diagramas desde texto |
| `chartjs` | Chart.js — gráficos desde JSON |
| `highlight` | Highlight.js — resaltado en cliente (opcional) |
| `anchor` | AnchorJS — enlaces ancla en encabezados |
| `fontawesome` | Font Awesome 6 — iconos |

### Chart.js

Dos formas de crear gráficos:

**1. Bloque `chart` (recomendado):** un bloque de código con lenguaje `chart`
y dentro un JSON de Chart.js. EngineMD genera el `<canvas>` y el script.

    ```chart
    {
      "type": "bar",
      "data": {
        "labels": ["Enero", "Febrero", "Marzo"],
        "datasets": [{"label": "Ventas", "data": [12, 19, 3]}]
      }
    }
    ```

Tipos: `bar`, `line`, `pie`, `doughnut`, `radar`, `polarArea`, `bubble`,
`scatter`.

**2. HTML directo:** `<canvas>` + `<script>` en el Markdown.

Si el JSON es inválido, el bloque se muestra como código normal.

## Soporte Obsidian

Se activa **automáticamente**, sin flags ni configuración, cuando el sitio
contiene una carpeta `.obsidian/` o cuando su contenido usa sintaxis Obsidian
(`[[...]]` o `![[...]]`). Se puede forzar con `"obsidian": true` global o por
sitio en `settings.json`. Si está desactivado, `[[...]]` se muestra literal.

### Wikilinks

| Sintaxis | Resultado |
|----------|-----------|
| `[[Nota]]` | Enlace a la nota (resuelto por nombre en todo el sitio) |
| `[[Nota\|texto]]` | Enlace con texto visible personalizado |
| `[[Nota#Encabezado]]` | Enlace a un encabezado (ancla generada) |
| `[[Nota#^id]]` | Enlace a un bloque marcado con `^id` |
| `[[carpeta/Nota]]` | Resolución por ruta relativa dentro de la bóveda |
| `[[Nota inexistente]]` | Enlace tenue (`wikilink-new`), sin error |

La resolución prioriza la ruta exacta, luego el nombre y, si hay varias
coincidencias, la ruta más corta.

### Embeds

| Sintaxis | Resultado |
|----------|-----------|
| `![[imagen.png]]` | Incrusta la imagen |
| `![[imagen.png\|640x480]]` | Incrusta la imagen con ancho y alto |
| `![[imagen.png\|100]]` | Incrusta la imagen con ancho (alto proporcional) |
| `![[Nota]]` | Incrusta el contenido completo de la nota |
| `![[Nota#Encabezado]]` | Incrusta solo esa sección |
| `![[Nota#^id]]` | Incrusta el bloque marcado con `^id` |

Las incrustaciones de notas se renderizan recursivamente con detección de
ciclos y límite de profundidad 5.

### Callouts

```markdown
> [!note] Título opcional
> Contenido del callout.
```

Tipos con color propio: `note`, `info`, `tip`, `hint`, `warning`, `important`,
`danger`, `error`.

### Bloques

Añade `^id` al final de un párrafo o elemento de lista para crear un ancla:

```markdown
Este párrafo es enlazable. ^mi-bloque
```

Luego usa `[[Otra nota#^mi-bloque]]` o `![[Otra nota#^mi-bloque]]`.

### Resolución de archivos

- Imágenes y adjuntos por nombre (con extensión).
- Notas por nombre (sin `.md`) o por ruta relativa.
- Se ignoran carpetas ocultas (`.obsidian/`, `.git/`), `node_modules/` y `target/`.
- El índice de la bóveda se cachea y se reconstruye en `--watch` al cambiar archivos.

## Extras de render

- **TOC** plegable y **botón de copiar** en bloques de código (cliente).
- **Compresión gzip** de las respuestas.
- **Healthcheck**: `GET /__enginemd/health`.
- Los enlaces a `.md` se renderizan como HTML (no se sirve el Markdown crudo).
- Los bloques de código conservan saltos de línea e indentación.

## Modo daemon

Ejecuta el servidor en segundo plano y lo mantiene tras reiniciar, sin
configurar un servicio.

```bash
enginemd daemon start      # arranca + autoarranque @reboot
enginemd daemon status
enginemd daemon logs
enginemd daemon stop       # detiene y desactiva el autoarranque
enginemd daemon restart
```

- **PID**: `~/.enginemd/enginemd.pid`; **log**: `~/.enginemd/enginemd.log`.
- Autoarranque: bloque `@reboot` en el crontab del usuario (se elimina con
  `daemon stop`; el resto del crontab se conserva).
- `daemon stop` solo termina el PID gestionado; si el pidfile es obsoleto, se
  limpia. Si `crontab` no está disponible, avisa pero arranca igual.
- Soporte solo Unix (Linux/macOS).

## Export estático

```bash
enginemd build --out ./public                # todos los sitios registrados
enginemd --path /ruta build --out ./public   # un solo directorio
```

Genera HTML en `DIR/<sitio>/...`, copia los assets a `DIR/__enginemd/` y
reescribe los enlaces `.md` a `.html`. Ideal para GitHub Pages u hosting
estático.

## Temas CSS

Hay 40+ temas. Los más usados:

| Tema | Descripción |
|------|-------------|
| `github` (default) | Estilo GitHub |
| `dark` | Modo oscuro |
| `simple` | Minimalista |

Se configuran por sitio (`css`) o con `--css-support`. La lista completa está
en `settings.json` (`styles`).

## Configuración global

`~/.enginemd/settings.json`:

```json
{
  "port": 10300,
  "watch_port": 9696,
  "lang": "en",
  "listing_css": "github",
  "listing_per_page": 20,
  "obsidian": false,
  "auto_fetch": true,
  "sri": true,
  "assets_dir": "/home/user/.enginemd",
  "cdn_base": "https://cdn.jsdelivr.net/npm",
  "cdn_fallbacks": ["https://unpkg.com"],
  "styles": { "github": "github.css", "dark": "dark.css", "simple": "simple.css" },
  "directories": [
    { "name": "docs", "path": "/home/user/docs", "active": true,
      "css": "dark", "js_support": ["mathjax", "mermaid"], "obsidian": null }
  ]
}
```

## Docker y CI

```bash
docker build -t enginemd .
docker run --rm -p 10300:10300 -v ~/.enginemd:/home/enginemd/.enginemd enginemd
```

El repositorio incluye CI de GitHub Actions (`.github/workflows/ci.yml`) que
ejecuta `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` y
`cargo build --release`.

## Limitaciones

- Los enlaces a notas Obsidian inexistentes no crean el archivo (enlace tenue).
- El enlace a encabezados usa un slug compatible con comrak; encabezados
  duplicados pueden resolver al primero.
- `![[archivo.pdf]]` y adjuntos no imagen se muestran como enlace de descarga.
- La primera ejecución (o `enginemd fetch`) necesita red una vez; después
  funciona offline.
