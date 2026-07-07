# Uso de Chart.js en EngineMD

## Habilitar Chart.js

Al crear un sitio:

```bash
enginemd new mi-sitio --js-support chartjs
```

O registrar uno existente:

```bash
enginemd up /ruta/al/proyecto --js-support chartjs
```

Para múltiples librerías:

```bash
enginemd up /ruta --js-support chartjs,mathjax,mermaid
```

## Método 1: Bloque `chart` (recomendado)

Escribe un bloque de código con el lenguaje `chart` y dentro un JSON con la configuración de Chart.js:

    ```chart
    {
      "type": "bar",
      "data": {
        "labels": ["Enero", "Febrero", "Marzo"],
        "datasets": [
          {
            "label": "Ventas",
            "data": [12, 19, 3]
          }
        ]
      }
    }
    ```

EngineMD genera automáticamente un `<canvas>` y el código JavaScript
necesario para crear el gráfico.

### Tipos de gráfico

| `type` | Descripción |
|--------|-------------|
| `bar` | Barras verticales |
| `line` | Líneas |
| `pie` | Pastel / circular |
| `doughnut` | Dona |
| `radar` | Radar |
| `polarArea` | Área polar |
| `bubble` | Burbujas |
| `scatter` | Dispersión |

### Ejemplos

**Gráfico de líneas con múltiples series:**

    ```chart
    {
      "type": "line",
      "data": {
        "labels": ["Ene", "Feb", "Mar", "Abr"],
        "datasets": [
          {"label": "Producto A", "data": [10, 15, 8, 12]},
          {"label": "Producto B", "data": [5, 20, 12, 18]}
        ]
      }
    }
    ```

**Gráfico de pastel:**

    ```chart
    {
      "type": "pie",
      "data": {
        "labels": ["Rojo", "Azul", "Verde"],
        "datasets": [{"data": [300, 50, 100]}]
      }
    }
    ```

**Con opciones avanzadas (título, colores, etc.):**

    ```chart
    {
      "type": "bar",
      "data": {
        "labels": ["A", "B", "C", "D"],
        "datasets": [{
          "label": "Datos",
          "data": [10, 20, 30, 40],
          "backgroundColor": ["#ff6384", "#36a2eb", "#cc65fe", "#ffce56"]
        }]
      },
      "options": {
        "responsive": true,
        "plugins": {
          "title": {
            "display": true,
            "text": "Gráfico de barras personalizado"
          },
          "legend": {
            "display": true,
            "position": "bottom"
          }
        },
        "scales": {
          "y": {
            "beginAtZero": true
          }
        }
      }
    }
    ```

## Método 2: HTML directo (avanzado)

Puedes insertar `<canvas>` y `<script>` directamente en el Markdown:

```markdown
<canvas id="mi-chart" style="max-height:300px"></canvas>
<script>
document.addEventListener('DOMContentLoaded', function() {
  var ctx = document.getElementById('mi-chart');
  if (!ctx) return;
  new Chart(ctx, {
    type: 'bar',
    data: {
      labels: ['A', 'B', 'C'],
      datasets: [{label: 'Serie', data: [1, 2, 3]}]
    }
  });
});
</script>
```

El HTML crudo se pasa directamente porque comrak lo permite (no hay
filtro de seguridad activado).

## Notas

- Chart.js se carga desde CDN (`cdn.jsdelivr.net`). Se necesita conexión
  a internet para que funcione.
- El bloque `chart` reemplaza al bloque de código, por lo que no se verá
  el JSON sin procesar en la página.
- Cada bloque genera un `<canvas>` con un ID único (`enginemd-chart-N`).
- Si el JSON es inválido, el bloque se muestra como código normal (sin
  renderizar).
- El `DOMContentLoaded` asegura que Chart.js esté cargado antes de
  ejecutar el código del gráfico.
