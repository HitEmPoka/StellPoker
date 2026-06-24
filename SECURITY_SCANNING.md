# Docker Image Security Scanning

This document describes the security scanning infrastructure for Docker images used in the StellPoker application.

## Overview

All Docker images are automatically scanned for security vulnerabilities:
- **Coordinator** - REST API service
- **MPC Node** - Multi-party computation node
- **Frontend** - Next.js web application

## Scanning Tools

### Trivy

[Trivy](https://github.com/aquasecurity/trivy) is the primary vulnerability scanner used in this project.

**Features:**
- Scans container images for vulnerabilities
- Detects vulnerabilities in OS packages and application dependencies
- Fast and lightweight scanning
- Integration with GitHub Security tab (SARIF format)
- Available as CLI tool and GitHub Action

**Installation:**
```bash
# macOS
brew install trivy

# Ubuntu/Debian
sudo apt-get install trivy

# Docker
docker run aquasec/trivy --version
```

### Snyk (Alternative)

[Snyk](https://snyk.io/) is available as an alternative for dependency scanning.

**Configuration:** See `.snyk` file for Snyk-specific settings.

## CI/CD Scanning

### Automated Workflow

The GitHub Actions workflow (`.github/workflows/docker-scan.yml`) runs:

1. **On every push to main** that modifies Dockerfile or app code
2. **On every pull request** that modifies Dockerfile or app code
3. **Weekly (Sunday 2 AM UTC)** for continuous monitoring

### Severity Levels

The CI pipeline fails on:
- **CRITICAL** vulnerabilities - Must be fixed immediately
- **HIGH** vulnerabilities - Should be fixed before merge

Lower severity vulnerabilities are reported but don't block merges.

### Scan Results

#### GitHub Security Tab
Results are uploaded in SARIF format and visible in:
1. GitHub -> Security -> Code scanning alerts
2. Pull request checks

#### Artifacts
Scan reports are stored as artifacts for 30 days:
- `coordinator-trivy-results.txt` - Table format
- `mpc-node-trivy-results.txt` - Table format
- `frontend-trivy-results.txt` - Table format
- `.sarif` files - Machine-readable format

#### PR Comments
Scan results are automatically commented on PRs showing a summary of findings per image.

## Local Scanning

### Running Scans Locally

Before pushing, scan your images locally:

```bash
./scripts/scan-docker-images.sh
```

This script:
1. Checks if Trivy is installed (installs if needed)
2. Builds all Docker images
3. Scans each image for CRITICAL and HIGH vulnerabilities
4. Reports summary and exits with appropriate code

### Individual Image Scanning

Scan a specific image:

```bash
# Build the image
docker build -f services/coordinator/Dockerfile -t coordinator:latest .

# Scan it
trivy image --severity CRITICAL,HIGH coordinator:latest
```

### Detailed Output

For full vulnerability details:

```bash
trivy image --severity CRITICAL,HIGH --format json coordinator:latest > scan-results.json
```

## Handling Vulnerabilities

### When Trivy Finds Vulnerabilities

1. **Analyze the vulnerability**
   - Check the CVE details and impact
   - Determine if it affects your use case
   - Check the base image version for available patches

2. **Fix the vulnerability**
   - Update the vulnerable package:
     ```dockerfile
     RUN apk add --no-cache --upgrade package-name
     ```
   - Use a newer base image version
   - Switch to a different package if no fix is available

3. **Verify the fix**
   ```bash
   ./scripts/scan-docker-images.sh
   ```

4. **Document the fix**
   - Include the CVE IDs in commit message
   - Link to the upstream fix or replacement

### False Positives

If a vulnerability is a false positive or unavoidable:

1. Document why in a comment in the Dockerfile
2. Consider adding a Snyk ignore in `.snyk`
3. Create an issue to track eventual fixing

Example:
```dockerfile
# CVE-2021-12345: False positive, package not used in runtime
RUN apk add --no-cache vulnerable-package
```

## Base Image Security

### Recommended Base Images

- **Rust services**: `rust:1.x-alpine` - Minimal, frequently updated
- **Node.js services**: `node:20-alpine` - Latest LTS, minimal
- **Minimal**: Alpine Linux is preferred over full distributions

### Updating Base Images

1. Check for new base image versions
2. Build locally and run `./scripts/scan-docker-images.sh`
3. Update `Dockerfile` with new version
4. Create PR for review

## Best Practices

1. **Keep images minimal** - Remove build tools from production stages
2. **Use multi-stage builds** - Separate build and runtime dependencies
3. **Run as non-root** - All containers should have a non-root user
4. **Regular updates** - Update base images and dependencies weekly
5. **Scan early** - Run local scans before pushing
6. **Monitor continuously** - The weekly scan catches new vulnerabilities

## Troubleshooting

### Trivy Not Found

```bash
# macOS
brew install trivy

# Ubuntu
sudo apt-get update && sudo apt-get install -y trivy

# Or use Docker
docker run aquasec/trivy image --version
```

### Scan Takes Too Long

- Trivy caches vulnerability database - first run is slower
- Subsequent scans are faster
- Use `trivy image --severity CRITICAL,HIGH` to skip lower severities

### Build Fails in CI

1. Check the SARIF file in artifacts
2. Review the PR comment with scan results
3. Fix vulnerabilities and push new commit

## References

- [Trivy Documentation](https://aquasecurity.github.io/trivy/)
- [Snyk Documentation](https://docs.snyk.io/)
- [Container Security Best Practices](https://kubernetes.io/docs/concepts/security/pod-security-standards/)
- [OWASP Container Security](https://owasp.org/www-project-container-security/)
