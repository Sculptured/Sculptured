import { useEffect, useRef, useState, type ComponentProps } from 'react';
import './text-mode.css';
import { useChatTextMode } from './ChatTextMode';

/** Shares the original field's value and validation; closing never discards text. */
export function TextArea(props: ComponentProps<'textarea'>) {
  const chat = useChatTextMode();
  const dialog = useRef<HTMLDialogElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const expanded = useRef<HTMLTextAreaElement>(null);
  const [title, setTitle] = useState('Text');
  const [opened, setOpened] = useState(false);
  useEffect(() => {
    if (!opened) return;
    dialog.current?.showModal();
    expanded.current?.focus();
  }, [opened]);
  const open = () => {
    const field = props.id ? document.getElementById(props.id) as HTMLTextAreaElement : null;
    setTitle(props['aria-label'] || field?.labels?.[0]?.textContent?.trim() || 'Text');
    setOpened(true);
  };
  const close = () => { dialog.current?.close(); setOpened(false); trigger.current?.focus(); };
  return <div className="text-field">
    <textarea {...props} />
    <button ref={trigger} type="button" className="text-mode-trigger" hidden={chat?.active} onClick={chat?.open ?? open} disabled={props.disabled}>Expand editor</button>
    {opened && <dialog ref={dialog} className="text-mode" role="dialog" aria-label="Expanded editor" onKeyDown={e => e.stopPropagation()} onCancel={e => { e.preventDefault(); close(); }}>
      <header><div><h2>{title}</h2><small>Expanded editor · edits stay in your original field</small></div><button type="button" className="btn" onClick={close}>Done</button></header>
      <textarea {...props} ref={expanded} id={undefined} aria-label={title} aria-describedby={undefined} onKeyDown={undefined} className="text-mode-editor" />
    </dialog>}
  </div>;
}
