import {
  Button,
  Dialog,
  DialogTrigger,
  Menu,
  MenuItem,
  MenuTrigger,
  Popover,
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
  useId,
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
  ...props
}: MenuTriggerProps): ReactElement | null {
  return <MenuTrigger {...props}>{children}</MenuTrigger>;
}

export function ActionMenuPopover({
  description,
  children,
  placement = "bottom end",
}: {
  readonly description?: ReactNode;
  readonly children: ReactNode;
  readonly placement?: PopoverProps["placement"];
}): ReactElement {
  const descriptionId = useId();
  return (
    <Popover
      className={styles.menuPopover!}
      placement={placement}
      offset={ANCHOR_OFFSET}
      containerPadding={VIEWPORT_PADDING}
      shouldFlip
    >
      <Menu
        className={styles.menu!}
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
    </Popover>
  );
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

export function ActionSubmenu({
  trigger,
  children,
}: {
  readonly trigger: ReactElement;
  readonly children: ReactNode;
}): ReactElement {
  return (
    <SubmenuTrigger delay={200}>
      {trigger}
      <Popover
        className={styles.menuPopover!}
        placement="start top"
        offset={ANCHOR_OFFSET}
        containerPadding={VIEWPORT_PADDING}
        shouldFlip
      >
        <Menu className={styles.menu!}>
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
