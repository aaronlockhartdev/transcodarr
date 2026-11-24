import createClient from 'openapi-fetch';
import type { paths } from './schema.ts';

export const api = createClient<paths>({ baseUrl: '/api' });
export type { components, paths } from './schema.ts';
