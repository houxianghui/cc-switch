import { z } from "zod";

export const timeWindowSchema = z
  .object({
    dow: z
      .array(z.number().int().min(0).max(6))
      .min(1, "schedule.error.emptyDow"),
    start: z.string().regex(/^([01]\d|2[0-3]):[0-5]\d$/),
    end: z.string().regex(/^([01]\d|2[0-3]):[0-5]\d$/),
  })
  .refine((w) => w.start < w.end, {
    message: "schedule.error.crossMidnight",
    path: ["end"],
  });

export const newRuleSchema = z.object({
  app: z.string().min(1),
  provider_id: z.string().min(1, "schedule.error.providerRequired"),
  windows: z.array(timeWindowSchema).min(1, "schedule.error.emptyWindows"),
  priority: z.number().int().min(0).max(1000),
  enabled: z.boolean(),
  note: z.string().nullable(),
});

export type NewRuleInput = z.infer<typeof newRuleSchema>;
