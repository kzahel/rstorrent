import {
  Button,
  Dialog,
  DialogTrigger,
  Header,
  Menu,
  MenuItem,
  MenuSection,
  MenuTrigger,
  Popover,
  RootMenuTriggerStateContext,
  Separator,
  SubmenuTrigger,
  type ButtonProps,
  type DialogProps,
  type DialogTriggerProps,
  type MenuItemProps,
  type MenuTriggerProps,
  type PopoverProps,
} from "react-aria-components";
import {
  forwardRef,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";

import styles from "./AnchoredOverlay.module.css";

const VIEWPORT_PADDING = 8;
const ANCHOR_OFFSET = 4;

export const OverlayButton = forwardRef<HTMLButtonElement, ButtonProps>(
  function OverlayButton(props, ref) {
    return <Button {...props} ref={ref} />;
  },
);

export function ActionMenuTrigger({
  children,
  isDisabled = false,
  isOpen: controlledOpen,
  onOpenChange,
  ...props
}: MenuTriggerProps & { readonly isDisabled?: boolean }): ReactElement | null {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  useEffect(() => {
    if (!isDisabled || !open) return;
    setUncontrolledOpen(false);
    onOpenChange?.(false);
  }, [isDisabled, onOpenChange, open]);

  return (
    <MenuTrigger
      {...props}
      isOpen={open}
      onOpenChange={(nextOpen) => {
        if (isDisabled && nextOpen) return;
        setUncontrolledOpen(nextOpen);
        onOpenChange?.(nextOpen);
      }}
    >
      {children}
    </MenuTrigger>
  );
}

export function ActionMenuPopover({
  label,
  description,
  children,
  placement = "bottom end",
}: {
  readonly label?: string;
  readonly description?: ReactNode;
  readonly children: ReactNode;
  readonly placement?: PopoverProps["placement"];
}): ReactElement {
  const descriptionId = useId();
  const triggerState = useContext(RootMenuTriggerStateContext);
  return (
    <Popover
      className={styles.menuPopover!}
      placement={placement}
      offset={ANCHOR_OFFSET}
      containerPadding={VIEWPORT_PADDING}
      shouldFlip
    >
      <div
        onKeyDownCapture={(event) => {
          if (event.key !== "Tab") return;
          event.preventDefault();
          const trigger = menuTriggerFor(event.currentTarget);
          triggerState?.close();
          requestAnimationFrame(() =>
            requestAnimationFrame(() =>
              moveFocusFrom(trigger, event.shiftKey ? -1 : 1),
            ),
          );
        }}
      >
        <Menu
          className={styles.menu!}
          {...(label === undefined ? {} : { "aria-label": label })}
          {...(description === undefined
            ? {}
            : { "aria-describedby": descriptionId })}
        >
          {children}
        </Menu>
        {description === undefined ? null : (
          <p id={descriptionId} className={styles.menuDescription!}>
            {description}
          </p>
        )}
      </div>
    </Popover>
  );
}

function menuTriggerFor(content: HTMLElement): HTMLElement | null {
  const triggerId = content
    .querySelector<HTMLElement>("[role='menu']")
    ?.getAttribute("aria-labelledby")
    ?.split(" ")[0];
  return triggerId === undefined
    ? null
    : document.getElementById(triggerId);
}

function moveFocusFrom(trigger: HTMLElement | null, direction: -1 | 1): void {
  if (trigger === null || !trigger.isConnected) return;
  const candidates = Array.from(
    document.querySelectorAll<HTMLElement>(
      'a[href], button, input, select, textarea, [tabindex]:not([tabindex="-1"])',
    ),
  ).filter(isTabbable);
  const triggerIndex = candidates.indexOf(trigger);
  if (triggerIndex < 0 || candidates.length === 0) return;
  const target =
    candidates[
      (triggerIndex + direction + candidates.length) % candidates.length
    ];
  target?.focus();
}

function isTabbable(element: HTMLElement): boolean {
  if (element.matches(":disabled, [aria-hidden='true'] *, [inert] *")) {
    return false;
  }
  const style = getComputedStyle(element);
  return style.display !== "none" && style.visibility !== "hidden";
}

export function ActionMenuItem({
  children,
  ...props
}: Omit<MenuItemProps, "children" | "className"> & {
  readonly children: ReactNode;
}): ReactElement {
  return (
    <MenuItem {...props} className={styles.menuItem!}>
      {children}
    </MenuItem>
  );
}

export function ActionMenuSection({
  label,
  children,
}: {
  readonly label: ReactNode;
  readonly children: ReactNode;
}): ReactElement {
  return (
    <MenuSection className={styles.menuSection!}>
      <Header className={styles.menuSectionHeader!}>{label}</Header>
      {children}
    </MenuSection>
  );
}

export function ActionMenuSeparator(): ReactElement {
  return <Separator className={styles.menuSeparator!} />;
}

export function ActionSubmenu({
  trigger,
  children,
  isDisabled = false,
}: {
  readonly trigger: ReactNode;
  readonly children: ReactNode;
  readonly isDisabled?: boolean;
}): ReactElement {
  const submenuRef = useRef<HTMLDivElement>(null);
  return (
    <SubmenuTrigger delay={200}>
      <MenuItem
        className={styles.menuItem!}
        isDisabled={isDisabled}
        onPress={() =>
          requestAnimationFrame(() => submenuRef.current?.focus())
        }
      >
        {trigger}
      </MenuItem>
      <Popover
        className={styles.menuPopover!}
        placement="start top"
        offset={ANCHOR_OFFSET}
        containerPadding={VIEWPORT_PADDING}
        shouldFlip
      >
        <Menu ref={submenuRef} className={styles.menu!}>
          {children}
        </Menu>
      </Popover>
    </SubmenuTrigger>
  );
}

export function AnchoredDialogTrigger({
  children,
  ...props
}: DialogTriggerProps): ReactElement {
  return <DialogTrigger {...props}>{children}</DialogTrigger>;
}

export function AnchoredDialog({
  children,
  className,
  placement = "bottom end",
  ...props
}: DialogProps & {
  readonly placement?: PopoverProps["placement"];
}): ReactElement {
  return (
    <Popover
      className={styles.dialogPopover!}
      placement={placement}
      offset={ANCHOR_OFFSET}
      containerPadding={VIEWPORT_PADDING}
      shouldFlip
    >
      <Dialog
        {...props}
        className={
          className === undefined
            ? styles.dialog!
            : `${styles.dialog!} ${className}`
        }
      >
        {children}
      </Dialog>
    </Popover>
  );
}
