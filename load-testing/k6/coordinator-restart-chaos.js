import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const errors = new Rate('chaos_errors');
const healthChecks = new Rate('health_checks');
const responseTime = new Trend('response_time_ms');

export const options = {
  scenarios: {
    active_load: {
      executor: 'constant-vus',
      vus: Number(__ENV.VUS || 25),
      duration: __ENV.DURATION || '2m',
    },
  },
  thresholds: {
    chaos_errors: ['rate<0.20'],
    health_checks: ['rate>0.80'],
    response_time_ms: ['p(99)<5000'],
  },
};

const baseUrl = (__ENV.BASE_URL || 'http://localhost:8080').replace(/\/$/, '');
const tableCount = Number(__ENV.TABLE_COUNT || 1000);

export default function () {
  const tableId = Math.floor(Math.random() * tableCount) + 1;
  const responses = http.batch([
    ['GET', `${baseUrl}/api/health`, null, { tags: { name: 'health' } }],
    ['GET', `${baseUrl}/api/tables/open`, null, { tags: { name: 'tables_open' } }],
    ['GET', `${baseUrl}/api/table/${tableId}/state`, null, { tags: { name: 'table_state' } }],
  ]);

  const healthOk = responses[0].status >= 200 && responses[0].status < 500;
  let failed = false;
  responses.forEach((response) => {
    responseTime.add(response.timings.duration);
    if (response.status >= 500 || response.status === 0) failed = true;
  });

  healthChecks.add(healthOk);
  errors.add(failed);
  check(responses[0], { 'health endpoint responds': (response) => response.status > 0 });
  sleep(1);
}
