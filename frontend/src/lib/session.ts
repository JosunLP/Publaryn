import { browser } from '$app/environment';
import { writable } from 'svelte/store';

import { clearAuthToken, getAuthToken } from '../api/client';

const tokenStore = writable<string | null>(browser ? getAuthToken() : null);

export const authToken = {
  subscribe: tokenStore.subscribe,
};

export function syncAuthToken(): void {
  tokenStore.set(browser ? getAuthToken() : null);
}

export function clearSession(): void {
  clearAuthToken();
  syncAuthToken();
}
