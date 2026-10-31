import { useEffect } from "react";
import { create } from "zustand";

import { call } from "../api/backend";
import { t } from "./i18n";

export interface PatchNote {
  title: string;
  version: string;
  kind: string;
  date: string;
  summary: string;
}

// Fetched once per session: the feed changes a few times a month.
const useNews = create<{ notes: PatchNote[] | null; asked: boolean }>(() => ({ notes: null, asked: false }));

export function usePatchNotes(): PatchNote[] | null {
  const notes = useNews((s) => s.notes);
  useEffect(() => {
    if (useNews.getState().asked) return;
    useNews.setState({ asked: true });
    call<PatchNote[]>("patch_notes").then(
      (list) => useNews.setState({ notes: list }),
      () => useNews.setState({ notes: [] }),
    );
  }, []);
  return notes;
}

/** The version's page on the Chinese Minecraft Wiki. */
export const wikiUrl = (version: string) => t(`https://zh.minecraft.wiki/w/Java版${encodeURIComponent(version)}`, `https://minecraft.wiki/w/Java_Edition_${encodeURIComponent(version)}`);
