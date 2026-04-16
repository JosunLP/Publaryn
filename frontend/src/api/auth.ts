import { api, clearAuthToken, setAuthToken } from './client';

export interface AuthResponse {
  token?: string | null;
  mfa_token?: string | null;
  [key: string]: unknown;
}

export interface RegisterInput {
  username: string;
  email: string;
  password: string;
}

export interface LoginInput {
  usernameOrEmail: string;
  password: string;
}

export interface MfaChallengeInput {
  mfaToken: string;
  code: string;
}

export interface UserProfile {
  username?: string | null;
  email?: string | null;
  display_name?: string | null;
  avatar_url?: string | null;
  website?: string | null;
  bio?: string | null;
  mfa_enabled?: boolean;
  [key: string]: unknown;
}

export interface UserUpdate {
  display_name?: string | null;
  avatar_url?: string | null;
  website?: string | null;
  bio?: string | null;
  [key: string]: unknown;
}

export interface MfaSetupState {
  secret: string;
  provisioning_uri: string;
  recovery_codes: string[];
}

export async function register(input: RegisterInput): Promise<AuthResponse> {
  const { data } = await api.post<AuthResponse>('/v1/auth/register', {
    body: {
      username: input.username,
      email: input.email,
      password: input.password,
    },
  });

  return data;
}

export async function login(input: LoginInput): Promise<AuthResponse> {
  const { data } = await api.post<AuthResponse>('/v1/auth/login', {
    body: {
      username_or_email: input.usernameOrEmail,
      password: input.password,
    },
  });

  if (data.token) {
    setAuthToken(data.token);
  }

  return data;
}

export async function completeMfaChallenge(
  input: MfaChallengeInput
): Promise<AuthResponse> {
  const { data } = await api.post<AuthResponse>('/v1/auth/mfa/challenge', {
    body: {
      mfa_token: input.mfaToken,
      code: input.code,
    },
  });

  if (data.token) {
    setAuthToken(data.token);
  }

  return data;
}

export async function logout(): Promise<void> {
  try {
    await api.post<null>('/v1/auth/logout');
  } finally {
    clearAuthToken();
  }
}

export async function getProfile(username: string): Promise<UserProfile> {
  const { data } = await api.get<UserProfile>(
    `/v1/users/${encodeURIComponent(username)}`
  );

  return data;
}

export async function getCurrentUser(): Promise<UserProfile> {
  const { data } = await api.get<UserProfile>('/v1/users/me');
  return data;
}

export async function updateCurrentUser(
  updates: UserUpdate
): Promise<UserProfile> {
  const { data } = await api.patch<UserProfile>('/v1/users/me', {
    body: updates,
  });

  return data;
}

export async function updateProfile(
  username: string,
  updates: UserUpdate
): Promise<UserProfile> {
  const { data } = await api.patch<UserProfile>(
    `/v1/users/${encodeURIComponent(username)}`,
    {
      body: updates,
    }
  );

  return data;
}

export async function setupMfa(): Promise<MfaSetupState> {
  const { data } = await api.post<MfaSetupState>('/v1/auth/mfa/setup');
  return data;
}

export async function verifyMfaSetup(
  code: string
): Promise<Record<string, unknown>> {
  const { data } = await api.post<Record<string, unknown>>(
    '/v1/auth/mfa/verify-setup',
    {
      body: { code },
    }
  );

  return data;
}

export async function disableMfa(
  code: string
): Promise<Record<string, unknown>> {
  const { data } = await api.post<Record<string, unknown>>(
    '/v1/auth/mfa/disable',
    {
      body: { code },
    }
  );

  return data;
}
