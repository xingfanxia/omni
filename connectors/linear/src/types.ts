export type LinearSyncState = {
  last_sync_at: string;
};

export type LinearSourceConfig = {
  team_keys?: string[];
};

export type LinearCredentials = {
  api_key: string;
};

export type LinearExtra = {
  linear: {
    team_id?: string | null;
    project_id?: string | null;
  };
};

export type LinearAttributes = {
  status?: string | null;
  priority?: string | null;
  labels?: string | null;
  assignee?: string | null;
  assignee_email?: string | null;
  team?: string | null;
  identifier?: string;
  project_name?: string | null;
  health?: string | null;
  lead?: string | null;
};
