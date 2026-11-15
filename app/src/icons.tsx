/** Line icons, one style: 16px box, 1.3 stroke, round ends. */
const icon = (d: string, cls = "icon", extra?: JSX.Element) => (
  <svg className={cls} viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
    <path d={d} />
    {extra}
  </svg>
);
export const FolderIcon = icon("M1.8 4.2c0-.6.4-1 1-1h3l1.4 1.5h6c.6 0 1 .4 1 1v6.6c0 .6-.4 1-1 1H2.8c-.6 0-1-.4-1-1z", "icon folder-icon");
export const PhraseIcon = icon("M4 1.8h5.2L12.5 5v8.2c0 .6-.4 1-1 1H4c-.6 0-1-.4-1-1V2.8c0-.6.4-1 1-1zM9 1.8V5h3.5M5.5 8.2h5M5.5 10.8h3.5");
export const TrashIcon = icon("M2.5 4.2h11M6.2 4.2V2.6h3.6v1.6M3.9 4.2l.7 9.2c0 .5.5.9 1 .9h4.8c.5 0 1-.4 1-.9l.7-9.2M6.6 6.8v4.8M9.4 6.8v4.8");
export const AccountIcon = icon("M2.8 14c.6-2.8 2.7-4.3 5.2-4.3s4.6 1.5 5.2 4.3", "icon", <circle cx="8" cy="5.5" r="2.7" />);
export const GearIcon = icon("M12.7 6.2L14.5 6.7L14.5 9.3L12.7 9.8L12.6 10.1L13.5 11.6L11.6 13.5L10.1 12.6L9.8 12.7L9.3 14.5L6.7 14.5L6.2 12.7L5.9 12.6L4.4 13.5L2.5 11.6L3.4 10.1L3.3 9.8L1.5 9.3L1.5 6.7L3.3 6.2L3.4 5.9L2.5 4.4L4.4 2.5L5.9 3.4L6.2 3.3L6.7 1.5L9.3 1.5L9.8 3.3L10.1 3.4L11.6 2.5L13.5 4.4L12.6 5.9Z", "icon", <circle cx="8" cy="8" r="2.1" />);
export const ImportIcon = icon("M8 2v8M4.8 7 8 10.2 11.2 7M2.5 11v1.7c0 .6.4 1 1 1h9c.6 0 1-.4 1-1V11");
export const ExportIcon = icon("M8 10.2V2M4.8 5.2 8 2l3.2 3.2M2.5 11v1.7c0 .6.4 1 1 1h9c.6 0 1-.4 1-1V11");
