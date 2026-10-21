# Frontend Content Security Policy Headers

## Overview
This document describes the Content Security Policy (CSP) headers implemented in the Next.js frontend (issue #136) to prevent XSS attacks, clickjacking, and other injection vulnerabilities.

## CSP Implementation

### Current Policy (Report-Only Mode)

The CSP headers are deployed in **report-only mode** to collect violations without blocking legitimate content. This allows us to identify necessary policy adjustments before enforcing the policy.

**Configuration Location**: `app/next.config.ts`

### Policy Directives

#### Core Directives

| Directive | Value | Purpose |
|---|---|---|
| `default-src` | `'self'` | Default fallback for all resources not explicitly covered. |
| `script-src` | `'self' 'unsafe-inline' 'unsafe-eval' https://cdn.jsdelivr.net` | Permits scripts from same origin, CDN, and inline (for Bootstrap/Next.js runtime). |
| `connect-src` | `'self' ws: wss:` | Allows fetch/XHR to same-origin APIs and WebSocket connections. |
| `style-src` | `'self' 'unsafe-inline' https://fonts.googleapis.com` | Permits styles from same-origin, inline, and Google Fonts. |
| `font-src` | `'self' https://fonts.gstatic.com` | Restricts fonts to same-origin and Google's font CDN. |
| `img-src` | `'self' data: https:` | Allows images from same-origin, data URIs, and HTTPS. |
| `frame-src` | `'none'` | Blocks embedding of frames (prevents clickjacking). |
| `form-action` | `'self'` | Forms can only submit to same-origin endpoints. |
| `base-uri` | `'self'` | `<base>` tag restricted to same-origin. |
| `object-src` | `'none'` | Blocks `<object>`, `<embed>`, `<applet>` tags. |

#### Additional Security Headers

| Header | Value | Purpose |
|---|---|---|
| `X-Frame-Options` | `DENY` | Prevents clickjacking by blocking framing. |
| `X-Content-Type-Options` | `nosniff` | Prevents MIME type sniffing attacks. |
| `X-XSS-Protection` | `1; mode=block` | Enable legacy XSS protection in older browsers. |
| `Referrer-Policy` | `strict-origin-when-cross-origin` | Minimal referrer leakage across origins. |
| `Permissions-Policy` | `camera=(), microphone=(), geolocation=()` | Denies access to sensitive browser APIs. |

## Migration to Enforcement Mode

When CSP report-only data shows no critical violations, migration to enforcement mode requires:

1. Update `app/next.config.ts`: replace `Content-Security-Policy-Report-Only` with `Content-Security-Policy`
2. Remove `'unsafe-inline'` and `'unsafe-eval'` from `script-src` if possible
3. Refactor any inline styles to CSS files (replace `'unsafe-inline'` in `style-src` if needed)
4. Monitor production metrics for CSP violations

### Enforcement Mode Policy (Future)

```
default-src 'self';
script-src 'self' https://cdn.jsdelivr.net;
connect-src 'self' ws: wss:;
style-src 'self' https://fonts.googleapis.com;
font-src 'self' https://fonts.gstatic.com;
img-src 'self' data: https:;
frame-src 'none';
form-action 'self';
base-uri 'self';
object-src 'none';
upgrade-insecure-requests;
```

## CSP Reporting

CSP violations are reported to the browser console and, if configured, to a CSP reporting endpoint. To enable server-side CSP violation monitoring:

```typescript
{
  key: "Content-Security-Policy",
  value: cspHeader + "; report-uri https://your-endpoint.example.com/csp-report",
}
```

## Testing CSP

### Local Testing

1. **Browser DevTools**: Open the Console tab and check for CSP violations as you interact with the app.
2. **Report-Only Mode**: The current deployment logs violations without blocking content.

### Automated CSP Testing

Add to your CI pipeline:

```bash
curl -I https://your-staging.example.com/ | grep -i content-security-policy
```

## Compliance Notes

- **OWASP**: Follows Level 3 CSP recommendations
- **GDPR**: No additional tracking/analytics domains are whitelisted in CSP
- **PCI-DSS**: Required for payment processing compliance

## Migration Timeline

- **Phase 1** (Current): Report-Only mode with monitoring
- **Phase 2**: Remove `'unsafe-inline'` from script-src (requires app refactoring)
- **Phase 3**: Enforcement mode activated

## Known Bypasses / Limitations

1. **`'unsafe-inline'` in `script-src`**: Allows inline scripts; requires removal before full enforcement
2. **`'unsafe-eval'`**: Required for some JavaScript runtime features; evaluate for removal
3. **CDN Whitelist**: If `cdn.jsdelivr.net` is compromised, malicious scripts could be injected

## References

- [MDN: Content-Security-Policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Security-Policy)
- [OWASP: Content Security Policy](https://cheatsheetseries.owasp.org/cheatsheets/Content_Security_Policy_Cheat_Sheet.html)
- [Next.js Security Headers](https://nextjs.org/docs/app/building-your-application/configuring/headers)
