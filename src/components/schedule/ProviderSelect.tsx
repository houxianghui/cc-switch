import { useTranslation } from "react-i18next";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppProviders } from "@/hooks/useAppProviders";
import type { AppId } from "@/lib/api";

interface Props {
  app: AppId;
  value: string | null;
  onChange: (id: string | null) => void;
}

export function ProviderSelect({ app, value, onChange }: Props) {
  const { t } = useTranslation();
  const { data } = useAppProviders(app);
  const providers = Object.values(data ?? {});
  return (
    <Select
      value={value ?? "__none__"}
      onValueChange={(v) => onChange(v === "__none__" ? null : v)}
    >
      <SelectTrigger>
        <SelectValue placeholder={t("schedule.provider")} />
      </SelectTrigger>
      <SelectContent>
        {providers.map((p) => (
          <SelectItem key={p.id} value={p.id}>
            {p.name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
