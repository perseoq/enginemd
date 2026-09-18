# Soporte Obsidian en EngineMD

EngineMD puede renderizar bóvedas de Obsidian respetando sus enlaces internos
(wikilinks) y sus incrustaciones (embeds).

## Activación

La detección es **automática y por defecto**, sin flags ni configuración. Un
sitio se trata como bóveda Obsidian cuando:

1. contiene una carpeta `.obsidian/` en su raíz, **o**
2. su contenido usa sintaxis Obsidian: algún `.md` incluye `[[...]]` o
   `![[...]]`.

La detección se cachea por sitio y se recalcula en modo `--watch` cuando
cambian los archivos.

Si necesitas forzar el comportamiento (por ejemplo, un sitio que usa `[[...]]`
sin ser Obsidian, o lo contrario), puedes usar `settings.json`, global o por
sitio:

```json
{
  "obsidian": true,
  "directories": [
    { "name": "mi-vault", "path": "/home/user/vault", "active": true, "obsidian": true }
  ]
}
```

Si el modo está desactivado, `[[...]]` se muestra como texto literal.

## Wikilinks

| Sintaxis | Resultado |
|----------|-----------|
| `[[Nota]]` | Enlace a la nota (resuelto por nombre en todo el sitio) |
| `[[Nota\|texto]]` | Enlace con texto visible personalizado |
| `[[Nota#Encabezado]]` | Enlace a un encabezado (ancla generada) |
| `[[Nota#^id]]` | Enlace a un bloque marcado con `^id` |
| `[[carpeta/Nota]]` | Resolución por ruta relativa dentro de la bóveda |
| `[[Nota inexistente]]` | Enlace tenue (`wikilink-new`), sin error |

La resolución prioriza la ruta exacta, luego el nombre del archivo y, si hay
varias coincidencias, la ruta más corta.

## Embeds

| Sintaxis | Resultado |
|----------|-----------|
| `![[imagen.png]]` | Incrusta la imagen |
| `![[imagen.png\|640x480]]` | Incrusta la imagen con ancho y alto |
| `![[imagen.png\|100]]` | Incrusta la imagen con ancho (alto proporcional) |
| `![[Nota]]` | Incrusta el contenido completo de la nota |
| `![[Nota#Encabezado]]` | Incrusta solo esa sección |
| `![[Nota#^id]]` | Incrusta el bloque marcado con `^id` |

Las incrustaciones de notas se renderizan recursivamente con detección de
ciclos y un límite de profundidad de 5.

## Bloques

Añade `^id` al final de un párrafo o elemento de lista para crear un ancla
enlazable:

```markdown
Este párrafo es enlazable. ^mi-bloque
```

Luego puedes referenciarlo con `[[Otra nota#^mi-bloque]]` o incrustarlo con
`![[Otra nota#^mi-bloque]]`.

## Resolución de archivos

- Las imágenes y adjuntos se resuelven por nombre (con extensión).
- Las notas se resuelven por nombre (sin `.md`) o por ruta relativa.
- Se ignoran las carpetas ocultas (`.obsidian/`, `.git/`, etc.), `node_modules/`
  y `target/`.
- El índice de la bóveda se cachea y se reconstruye automáticamente en modo
  `--watch` cuando cambian los archivos.

## Limitaciones

- Los enlaces a notas inexistentes no crean el archivo (solo se muestran en
  color tenue).
- El enlace a encabezados usa un slug compatible con comrak; encabezados
  duplicados pueden resolver al primero.
- `![[archivo.pdf]]` y adjuntos no imagen se muestran como enlace de descarga.
