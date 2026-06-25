import http from 'k6/http';
import { Rate, Trend } from 'k6/metrics';
import { sleep } from 'k6';

const errorRate = new Rate('errors');
const responseTime = new Trend('response_time_ms');

export let options = {
  vus: __ENV.VUS ? parseInt(__ENV.VUS, 10) : 100,
  duration: __ENV.DURATION || '3m',
  thresholds: {
    errors: ['rate<0.05'],
    response_time_ms: ['p(99)<2000'],
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://host.docker.internal:8080';
const TABLE_COUNT = __ENV.TABLE_COUNT ? parseInt(__ENV.TABLE_COUNT, 10) : 1000;

function randomTableId() {
  return Math.floor(Math.random() * TABLE_COUNT) + 1;
}

export default function () {
  const tableId = randomTableId();
  const requests = [
    ['GET', `${BASE_URL}/api/health`, null, { tags: { name: 'health' } }],
    ['GET', `${BASE_URL}/api/tables/open`, null, { tags: { name: 'tables_open' } }],
    ['GET', `${BASE_URL}/api/table/${tableId}/state`, null, { tags: { name: 'table_state' } }],
  ];

  const responses = http.batch(requests);
  let hasError = false;

  responses.forEach((res) => {
    if (res.status >= 500) {
      hasError = true;
    }
    responseTime.add(res.timings.duration);
  });

  errorRate.add(hasError);
  sleep(1);
}
