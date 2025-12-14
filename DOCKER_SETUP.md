# Docker Hub Deployment Setup

This guide describes how to set up automatic Docker publishing to Docker Hub.

## Prerequisites

1. A Docker Hub account (<https://hub.docker.com/>)
2. Repository admin access on GitHub

## Step 1: Create Docker Hub Access Token

1. Go to <https://hub.docker.com/settings/security>
2. Click "New Access Token"
3. Enter a name (e.g., "github-actions-sort-it-now")
4. Select "Read, Write" permission
5. Click "Generate"
6. **Important:** Copy the token immediately - it will only be shown once!

## Step 2: Configure GitHub Secrets

1. Go to your GitHub repository
2. Navigate to **Settings** → **Secrets and variables** → **Actions**
3. Click "New repository secret"
4. Create two secrets:

   **Secret 1:**

   - Name: `DOCKER_USERNAME`
   - Value: Your Docker Hub username

   **Secret 2:**

   - Name: `DOCKER_PASSWORD`
   - Value: The access token from Step 1

## Step 3: Test the Workflow

The Docker workflow is automatically triggered when:

- A new tag in the format `v*` is created (e.g., `v1.1.0`)
- The workflow is manually started via "Actions" → "Docker Build and Push" → "Run workflow"

### Manual Test

1. Go to **Actions** in the GitHub repository
2. Select the workflow "Docker Build and Push"
3. Click "Run workflow"
4. Select the branch
5. Click "Run workflow"

## Step 4: Verify Docker Image on Docker Hub

After successful workflow completion:

1. Go to <https://hub.docker.com/>
2. Navigate to your repository
3. The image should be available with the corresponding tags:
   - `latest` (assigned with each release having a `v*` tag)
   - Version tags (e.g., `1.0.0`, `1.0`, `1`)

## Using the Docker Image

After publishing, the image can be used as follows:

> **Note:** Replace `<your-dockerhub-username>` with your actual Docker Hub username.

```bash
# Latest version
docker pull <your-dockerhub-username>/sort-it-now:latest

# Specific version
docker pull <your-dockerhub-username>/sort-it-now:1.0.0

# Run
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 <your-dockerhub-username>/sort-it-now:latest
```

## Troubleshooting

### Workflow fails with "Authentication failed"

- Check if the secrets are set correctly
- Ensure the Docker Hub access token has not expired
- Verify the Docker Hub username (case-sensitive)

### Workflow fails with "denied: requested access to the resource is denied"

- The access token needs "Write" permission
- Ensure the repository exists on Docker Hub (automatically created on first push)

### Image is not built for all platforms

- Docker Buildx is automatically set up
- If there are issues, you can reduce the line `platforms: linux/amd64,linux/arm64` in `.github/workflows/docker.yml` to just `linux/amd64`

## Customizations

### Change Docker Hub Repository Name

In `.github/workflows/docker.yml`, change the line:

```yaml
images: ${{ secrets.DOCKER_USERNAME }}/sort-it-now
```

to:

```yaml
images: ${{ secrets.DOCKER_USERNAME }}/your-repository-name
```

### Use Different Registry (e.g., GitHub Container Registry)

For GitHub Container Registry (ghcr.io):

1. Replace `docker/login-action` with GitHub Token:

```yaml
- name: Log in to GitHub Container Registry
  uses: docker/login-action@v3
  with:
    registry: ghcr.io
    username: ${{ github.actor }}
    password: ${{ secrets.GITHUB_TOKEN }}
```

2. Change the image in `metadata-action`:

```yaml
images: ghcr.io/${{ github.repository_owner }}/sort-it-now
```
