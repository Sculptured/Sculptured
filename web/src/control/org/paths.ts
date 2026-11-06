/** The organisation overview's addresses (Control room). */
export const ORG_CONTROL_PATH = '/control';
const e = encodeURIComponent;
/** Where a popped-out Control room pane opens. */
export const orgPanePath = (instance: string) => `${ORG_CONTROL_PATH}/pane/${e(instance)}`;
