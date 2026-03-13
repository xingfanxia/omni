import type { SyncContext } from './context.js';
import type {
  ConnectorManifest,
  ActionDefinition,
  ActionResponse,
} from './models.js';
import { createActionResponseNotSupported } from './models.js';
import { createServer } from './server.js';

export interface ServeOptions {
  port?: number;
  host?: string;
}

export abstract class Connector {
  abstract readonly name: string;
  abstract readonly version: string;

  get displayName(): string {
    return this.name;
  }

  readonly syncModes: string[] = ['full'];
  readonly actions: ActionDefinition[] = [];

  getManifest(): ConnectorManifest {
    return {
      name: this.name,
      display_name: this.displayName,
      version: this.version,
      sync_modes: this.syncModes,
      actions: this.actions,
    };
  }

  abstract sync(
    sourceConfig: Record<string, unknown>,
    credentials: Record<string, unknown>,
    state: Record<string, unknown> | null,
    ctx: SyncContext
  ): Promise<void>;

  cancel(_syncRunId: string): boolean {
    return false;
  }

  executeAction(
    action: string,
    _params: Record<string, unknown>,
    _credentials: Record<string, unknown>
  ): Promise<ActionResponse> {
    return Promise.resolve(createActionResponseNotSupported(action));
  }

  serve(options: ServeOptions = {}): void {
    const port = options.port ?? parseInt(process.env.PORT ?? '8000', 10);
    const host = options.host ?? '0.0.0.0';

    const app = createServer(this);
    app.listen(port, host, () => {
      console.log(`Connector ${this.name} v${this.version} listening on ${host}:${port}`);
    });
  }
}
