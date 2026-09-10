import { useTranslation } from "react-i18next";
import { useFieldArray, type UseFormReturn } from "react-hook-form";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import type { NewRuleInput } from "@/lib/schemas/schedule";

const DOW = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"] as const;

/** A fresh object per call, so appended windows never share a `dow` array. */
export function createDefaultWindow(): NewRuleInput["windows"][number] {
  return { dow: [1, 2, 3, 4, 5], start: "09:00", end: "18:00" };
}

interface Props {
  form: UseFormReturn<NewRuleInput>;
}

export function WindowListEditor({ form }: Props) {
  const { t } = useTranslation();
  const { fields, append, remove } = useFieldArray({
    control: form.control,
    name: "windows",
  });

  return (
    <div className="space-y-2">
      <div className="text-sm font-medium">{t("schedule.windows")}</div>
      {fields.map((field, i) => (
        <div key={field.id} className="rounded-md border p-3 space-y-3">
          <div className="flex flex-wrap gap-x-3 gap-y-2">
            {DOW.map((day, dayIndex) => (
              <label key={day} className="flex items-center gap-1.5 text-xs">
                <Checkbox
                  checked={
                    form.watch(`windows.${i}.dow`)?.includes(dayIndex) ?? false
                  }
                  onCheckedChange={(checked) => {
                    const current = form.getValues(`windows.${i}.dow`) ?? [];
                    const next = checked
                      ? [...new Set([...current, dayIndex])].sort(
                          (a, b) => a - b,
                        )
                      : current.filter((d) => d !== dayIndex);
                    form.setValue(`windows.${i}.dow`, next, {
                      shouldDirty: true,
                    });
                  }}
                />
                {t(`schedule.dow.${day}`)}
              </label>
            ))}
          </div>
          <div className="flex items-center gap-2">
            <Input
              type="time"
              className="flex-1"
              {...form.register(`windows.${i}.start`)}
            />
            <span className="text-muted-foreground">–</span>
            <Input
              type="time"
              className="flex-1"
              {...form.register(`windows.${i}.end`)}
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label={t("schedule.removeWindow")}
              onClick={() => remove(i)}
            >
              <Trash2 className="size-4" />
            </Button>
          </div>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => append(createDefaultWindow())}
      >
        <Plus className="size-3.5 mr-1" />
        {t("schedule.addWindow")}
      </Button>
    </div>
  );
}
