import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';
const TABLE_ID = __ENV.TABLE_ID;
const POLL_TIMEOUT_SECONDS = Number(__ENV.POLL_TIMEOUT_SECONDS || 15);
const POLL_INTERVAL_MS = Number(__ENV.POLL_INTERVAL_MS || 250);
const AUTH_HEADERS = __ENV.AUTH_HEADERS_JSON
  ? JSON.parse(__ENV.AUTH_HEADERS_JSON)
  : {};

const dealSuccess = new Counter('deal_success');
const dealAborted = new Counter('deal_aborted');
const dealUnexpected = new Counter('deal_unexpected');
const sessionCompleted = new Counter('session_completed');
const sessionAborted = new Counter('session_aborted');

if (!TABLE_ID) {
  throw new Error('TABLE_ID is required');
}

export const options = {
  vus: Number(__ENV.VUS || 1),
  duration: __ENV.DURATION || '2m',
  thresholds: {
    checks: ['rate==1'],
    deal_unexpected: ['count==0'],
    deal_success: ['count>0'],
  },
};

function headers() {
  return Object.assign({ 'Content-Type': 'application/json' }, AUTH_HEADERS);
}

function pollSession(sessionId) {
  const deadline = Date.now() + POLL_TIMEOUT_SECONDS * 1000;
  while (Date.now() < deadline) {
    const response = http.get(`${BASE_URL}/api/session/${encodeURIComponent(sessionId)}/status`, {
      headers: headers(),
    });
    if (response.status !== 200) {
      sleep(POLL_INTERVAL_MS / 1000);
      continue;
    }
    let body;
    try {
      body = JSON.parse(response.body);
    } catch (e) {
      sleep(POLL_INTERVAL_MS / 1000);
      continue;
    }
    const status = String(body.status || '').toLowerCase();
    if (['completed', 'failed', 'timed_out', 'cancelled'].includes(status)) {
      return status;
    }
    sleep(POLL_INTERVAL_MS / 1000);
  }
  return 'timeout';
}

export default function () {
  sleep((__VU % 5) * 0.2 + Math.random() * 0.2);

  const response = http.post(
    `${BASE_URL}/api/table/${encodeURIComponent(TABLE_ID)}/request-deal`,
    '{}',
    { headers: headers(), timeout: '15s' },
  );

  const accepted = check(response, {
    'deal request accepted or safely refused': (r) => [200, 409, 503].includes(r.status),
  });
  if (!accepted) {
    dealUnexpected.add(1);
    return;
  }

  if (response.status === 200) {
    dealSuccess.add(1);
    let sessionId;
    try {
      sessionId = JSON.parse(response.body).session_id;
    } catch (e) {
      sessionId = undefined;
    }
    if (!sessionId) {
      dealUnexpected.add(1);
      return;
    }
    const status = pollSession(sessionId);
    if (status === 'completed') {
      sessionCompleted.add(1);
    } else {
      sessionAborted.add(1);
    }
    check(null, {
      'session terminates within timeout': () => status !== 'timeout',
    });
  } else {
    dealAborted.add(1);
    sessionAborted.add(1);
  }
}
