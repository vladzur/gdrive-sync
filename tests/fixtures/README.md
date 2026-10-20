# Fixtures de Google Drive

Respuestas de la API v3 capturadas una sola vez y anonimizadas, usadas por los
tests con `wiremock` (ver §8.2 del plan de implementación).

Reglas:

1. **CI nunca llama a Google.** Todo el tráfico HTTP de los tests se resuelve
   con `wiremock` contra estos archivos.
2. **Anonimizar antes de versionar**: `fileId` → `file-<n>`, nombres →
   `sample-<n>`, y eliminar `emailAddress`, `displayName` y cualquier dato
   personal.
3. Un archivo por endpoint o escenario, en JSON con formato estable, para que
   los diffs sean legibles.
