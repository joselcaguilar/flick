import * as ToastPrimitive from "@radix-ui/react-toast";
import { createContext, type ReactNode, useCallback, useContext, useMemo, useState } from "react";

type ToastTone = "info" | "success" | "warning" | "danger";
interface ToastItem {
  id: number;
  title: string;
  description?: string;
  tone: ToastTone;
}

const ToastContext = createContext<{ show: (toast: Omit<ToastItem, "id">) => void } | null>(null);

export function useToast() {
  const context = useContext(ToastContext);
  if (!context) throw new Error("useToast must be used inside ToastProvider");
  return context;
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const show = useCallback((toast: Omit<ToastItem, "id">) => {
    const id = Date.now();
    setToasts((current) => [...current, { ...toast, id }]);
  }, []);
  const value = useMemo(() => ({ show }), [show]);

  return (
    <ToastContext.Provider value={value}>
      <ToastPrimitive.Provider swipeDirection="right">
        {children}
        {toasts.map((toast) => (
          <ToastPrimitive.Root
            key={toast.id}
            className="ui-toast"
            data-tone={toast.tone}
            onOpenChange={(open) =>
              !open && setToasts((current) => current.filter((item) => item.id !== toast.id))
            }
          >
            <ToastPrimitive.Title>{toast.title}</ToastPrimitive.Title>
            {toast.description ? (
              <ToastPrimitive.Description>{toast.description}</ToastPrimitive.Description>
            ) : null}
          </ToastPrimitive.Root>
        ))}
        <ToastPrimitive.Viewport className="ui-toast-viewport" />
      </ToastPrimitive.Provider>
    </ToastContext.Provider>
  );
}
