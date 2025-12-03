# Docker Hub Deployment Setup

Diese Anleitung beschreibt, wie man die automatische Docker-Veröffentlichung auf Docker Hub einrichtet.

## Voraussetzungen

1. Ein Docker Hub Account (https://hub.docker.com/)
2. Repository-Admin-Zugriff auf GitHub

## Schritt 1: Docker Hub Access Token erstellen

1. Gehe zu https://hub.docker.com/settings/security
2. Klicke auf "New Access Token"
3. Gib einen Namen ein (z.B. "github-actions-sort-it-now")
4. Wähle die Berechtigung "Read, Write, Delete" aus
5. Klicke auf "Generate"
6. **Wichtig:** Kopiere das Token sofort - es wird nur einmal angezeigt!

## Schritt 2: GitHub Secrets konfigurieren

1. Gehe zu deinem GitHub Repository
2. Navigiere zu **Settings** → **Secrets and variables** → **Actions**
3. Klicke auf "New repository secret"
4. Erstelle zwei Secrets:

   **Secret 1:**
   - Name: `DOCKER_USERNAME`
   - Value: Dein Docker Hub Benutzername

   **Secret 2:**
   - Name: `DOCKER_PASSWORD`
   - Value: Das Access Token aus Schritt 1

## Schritt 3: Workflow testen

Der Docker-Workflow wird automatisch ausgelöst, wenn:
- Ein neuer Tag im Format `v*` erstellt wird (z.B. `v1.0.0`)
- Der Workflow manuell über "Actions" → "Docker Build and Push" → "Run workflow" gestartet wird

### Manueller Test:

1. Gehe zu **Actions** im GitHub Repository
2. Wähle den Workflow "Docker Build and Push"
3. Klicke auf "Run workflow"
4. Wähle den Branch aus
5. Klicke auf "Run workflow"

## Schritt 4: Docker Image auf Docker Hub verifizieren

Nach erfolgreichem Workflow-Durchlauf:

1. Gehe zu https://hub.docker.com/
2. Navigiere zu deinem Repository
3. Das Image sollte mit den entsprechenden Tags verfügbar sein:
   - `latest` (für den Standard-Branch)
   - Versions-Tags (z.B. `1.0.0`, `1.0`, `1`)

## Docker Image verwenden

Nach der Veröffentlichung kann das Image folgendermaßen verwendet werden:

```bash
# Neueste Version
docker pull your-dockerhub-username/sort-it-now:latest

# Spezifische Version
docker pull your-dockerhub-username/sort-it-now:1.0.0

# Ausführen
docker run -p 8080:8080 your-dockerhub-username/sort-it-now:latest
```

## Troubleshooting

### Workflow schlägt mit "Authentication failed" fehl
- Überprüfe, ob die Secrets korrekt gesetzt sind
- Stelle sicher, dass das Docker Hub Access Token nicht abgelaufen ist
- Verifiziere den Docker Hub Benutzernamen (Groß-/Kleinschreibung beachten)

### Workflow schlägt mit "denied: requested access to the resource is denied" fehl
- Das Access Token benötigt "Write"-Berechtigung
- Stelle sicher, dass das Repository auf Docker Hub existiert (wird automatisch beim ersten Push erstellt)

### Image wird nicht mit allen Plattformen gebaut
- Docker Buildx wird automatisch eingerichtet
- Bei Problemen kann man in `.github/workflows/docker.yml` die Zeile `platforms: linux/amd64,linux/arm64` auf nur `linux/amd64` reduzieren

## Anpassungen

### Docker Hub Repository-Name ändern

In `.github/workflows/docker.yml` die Zeile:
```yaml
images: ${{ secrets.DOCKER_USERNAME }}/sort-it-now
```

ändern zu:
```yaml
images: ${{ secrets.DOCKER_USERNAME }}/dein-repository-name
```

### Andere Registry verwenden (z.B. GitHub Container Registry)

Für GitHub Container Registry (ghcr.io):
1. Ersetze `docker/login-action` mit GitHub Token:
```yaml
- name: Log in to GitHub Container Registry
  uses: docker/login-action@v3
  with:
    registry: ghcr.io
    username: ${{ github.actor }}
    password: ${{ secrets.GITHUB_TOKEN }}
```

2. Ändere das Image in `metadata-action`:
```yaml
images: ghcr.io/${{ github.repository_owner }}/sort-it-now
```
