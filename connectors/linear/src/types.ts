export interface LinearSyncState {
  last_sync_at: string;
}

export interface LinearSourceConfig {
  team_keys?: string[];
}

export interface LinearCredentials {
  api_key: string;
}
