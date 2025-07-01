/** @noSelfInFile */
/** @noResolution */
declare module "event_handler" {
  import { ConfigurationChangedData, NthTickEventData } from "factorio:runtime"

  type Events = {
    [K in defines.events]?: (this: void, event: K["_eventData"]) => void
  }

  /** @noSelf */
  export interface EventLib {
    events?: Events
    on_nth_tick?: Record<number, (this: void, event: NthTickEventData) => void>

    on_init?(): void

    on_load?(): void

    on_configuration_changed?(data: ConfigurationChangedData): void
  }

  /** @noSelf */
  export function add_lib(lib: EventLib): void

  /** @noSelf */
  export function add_libraries(libs: EventLib[]): void
}
