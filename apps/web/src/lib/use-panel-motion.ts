import { useGSAP } from "@gsap/react";
import { gsap } from "gsap";
import type { RefObject } from "react";

import type { SessionRecord } from "./types";
import { sessionKey, sessionRef } from "./types";

gsap.registerPlugin(useGSAP);

interface PanelMotionState {
  root: RefObject<HTMLDivElement | null>;
  connectionTone: string;
  deviceFilter: string | undefined;
  mobileFiltersOpen: boolean;
  selectedKey: string | undefined;
  settingsOpen: boolean;
  tab: string;
  timeline: SessionRecord[] | undefined;
  visibleSessions: SessionRecord[];
}

const NO_MOTION = "(prefers-reduced-motion: no-preference)";

export function usePanelMotion({
  root,
  connectionTone,
  deviceFilter,
  mobileFiltersOpen,
  selectedKey,
  settingsOpen,
  tab,
  timeline,
  visibleSessions,
}: PanelMotionState) {
  const visibleMotionKey = visibleSessions
    .map(
      (session) =>
        `${sessionKey(sessionRef(session))}:${session.payload.state.activity_status}`,
    )
    .join("|");
  const timelineMotionKey =
    timeline?.map((event) => event.event_id).join("|") ?? "";

  useGSAP(
    () => {
      const panel = root.current;
      if (panel === null) {
        return;
      }
      const media = gsap.matchMedia();

      media.add(NO_MOTION, () => {
        const brandCopy = Array.from(
          panel.querySelectorAll(".brand-block > *"),
        );
        const headerActions = Array.from(
          panel.querySelectorAll(".header-actions > *"),
        );
        const overviewCards = Array.from(
          panel.querySelectorAll(".overview-card"),
        );
        const controlSurface = panel.querySelector(".control-surface");
        const listHeader = panel.querySelector(".list-header");
        const timeline = gsap.timeline({
          defaults: { duration: 0.58, ease: "power3.out" },
        });

        timeline
          .from(brandCopy, {
            autoAlpha: 0,
            y: 18,
            stagger: 0.065,
            clearProps: "opacity,transform,visibility",
          })
          .from(
            headerActions,
            {
              autoAlpha: 0,
              x: 16,
              stagger: 0.06,
              clearProps: "opacity,transform,visibility",
            },
            "<0.08",
          )
          .from(
            overviewCards,
            {
              autoAlpha: 0,
              y: 24,
              scale: 0.975,
              stagger: 0.075,
              clearProps: "opacity,transform,visibility",
            },
            "-=0.24",
          );

        if (controlSurface !== null) {
          timeline.from(
            controlSurface,
            {
              autoAlpha: 0,
              y: 18,
              clearProps: "opacity,transform,visibility",
            },
            "-=0.34",
          );
        }
        if (listHeader !== null) {
          timeline.from(
            listHeader,
            {
              autoAlpha: 0,
              y: 12,
              clearProps: "opacity,transform,visibility",
            },
            "-=0.34",
          );
        }

        const brandMark = panel.querySelector(".brand-mark");
        const brandSignal = panel.querySelector(".brand-mark-signal");
        if (brandMark !== null) {
          gsap.to(brandMark, {
            rotation: 360,
            duration: 22,
            ease: "none",
            repeat: -1,
          });
        }
        if (brandSignal !== null) {
          gsap.to(brandSignal, {
            autoAlpha: 0.48,
            scale: 1.5,
            duration: 1.25,
            ease: "sine.inOut",
            repeat: -1,
            yoyo: true,
          });
        }
      });

      media.add(
        "(hover: hover) and (pointer: fine) and (prefers-reduced-motion: no-preference)",
        () => {
          const cleanups = Array.from(
            panel.querySelectorAll<HTMLElement>(".overview-card"),
          ).map((card) => {
            const moveX = gsap.quickTo(card, "x", {
              duration: 0.42,
              ease: "power3.out",
            });
            const moveY = gsap.quickTo(card, "y", {
              duration: 0.42,
              ease: "power3.out",
            });
            const onPointerMove = (event: PointerEvent) => {
              const bounds = card.getBoundingClientRect();
              moveX(((event.clientX - bounds.left) / bounds.width - 0.5) * 8);
              moveY(((event.clientY - bounds.top) / bounds.height - 0.5) * 8);
            };
            const onPointerLeave = () => {
              moveX(0);
              moveY(0);
            };
            card.addEventListener("pointermove", onPointerMove);
            card.addEventListener("pointerleave", onPointerLeave);
            return () => {
              card.removeEventListener("pointermove", onPointerMove);
              card.removeEventListener("pointerleave", onPointerLeave);
            };
          });
          return () => {
            cleanups.forEach((cleanup) => cleanup());
          };
        },
      );

      return () => media.revert();
    },
    { scope: root },
  );

  useGSAP(
    () => {
      const panel = root.current;
      if (panel === null) {
        return;
      }
      const media = gsap.matchMedia();
      media.add(NO_MOTION, () => {
        const cards = Array.from(panel.querySelectorAll(".session-card"));
        if (cards.length === 0) {
          return;
        }
        gsap.fromTo(
          cards,
          { autoAlpha: 0, y: 18, scale: 0.988 },
          {
            autoAlpha: 1,
            y: 0,
            scale: 1,
            duration: 0.48,
            ease: "power3.out",
            stagger: { each: 0.055, from: "start" },
            clearProps: "opacity,transform,visibility",
          },
        );
      });
      return () => media.revert();
    },
    {
      dependencies: [visibleMotionKey],
      scope: root,
      revertOnUpdate: true,
    },
  );

  useGSAP(
    () => {
      const panel = root.current;
      const detail = panel?.querySelector(".detail");
      if (detail === null || detail === undefined) {
        return;
      }
      const media = gsap.matchMedia();
      media.add(
        {
          desktop: "(min-width: 920px)",
          mobile: "(max-width: 919px)",
          motion: NO_MOTION,
        },
        (context) => {
          if (!context.conditions?.motion) {
            return;
          }
          const fromMobile = context.conditions.mobile;
          const detailSections = Array.from(
            detail.querySelectorAll(
              ".detail-header, .detail-state, .timeline > h3",
            ),
          );
          const motion = gsap.timeline({
            defaults: { ease: "power3.out" },
          });
          motion
            .fromTo(
              detail,
              {
                autoAlpha: 0,
                x: fromMobile ? 0 : 28,
                y: fromMobile ? 28 : 0,
                scale: fromMobile ? 0.992 : 1,
              },
              {
                autoAlpha: 1,
                x: 0,
                y: 0,
                scale: 1,
                duration: 0.46,
                clearProps: "opacity,transform,visibility",
              },
            )
            .from(
              detailSections,
              {
                autoAlpha: 0,
                y: 10,
                duration: 0.34,
                stagger: 0.045,
                clearProps: "opacity,transform,visibility",
              },
              "-=0.24",
            );
        },
      );
      return () => media.revert();
    },
    {
      dependencies: [selectedKey],
      scope: root,
      revertOnUpdate: true,
    },
  );

  useGSAP(
    () => {
      const panel = root.current;
      if (panel === null || timelineMotionKey.length === 0) {
        return;
      }
      const media = gsap.matchMedia();
      media.add(NO_MOTION, () => {
        const items = Array.from(panel.querySelectorAll(".timeline-item"));
        gsap.fromTo(
          items,
          { autoAlpha: 0, x: 12 },
          {
            autoAlpha: 1,
            x: 0,
            duration: 0.36,
            ease: "power2.out",
            stagger: 0.04,
            clearProps: "opacity,transform,visibility",
          },
        );
      });
      return () => media.revert();
    },
    {
      dependencies: [timelineMotionKey],
      scope: root,
      revertOnUpdate: true,
    },
  );

  useGSAP(
    () => {
      const panel = root.current;
      if (panel === null) {
        return;
      }
      const media = gsap.matchMedia();
      media.add(NO_MOTION, () => {
        const backdrops = Array.from(
          panel.querySelectorAll<HTMLElement>(
            ".mobile-sheet-backdrop, .settings-backdrop",
          ),
        );
        backdrops.forEach((backdrop) => {
          const surface = backdrop.querySelector<HTMLElement>(
            ".mobile-sheet, .settings",
          );
          gsap.fromTo(
            backdrop,
            { autoAlpha: 0 },
            {
              autoAlpha: 1,
              duration: 0.2,
              ease: "power1.out",
              clearProps: "opacity,visibility",
            },
          );
          if (surface !== null) {
            gsap.fromTo(
              surface,
              { autoAlpha: 0, y: 34, scale: 0.985 },
              {
                autoAlpha: 1,
                y: 0,
                scale: 1,
                duration: 0.42,
                ease: "power3.out",
                clearProps: "opacity,transform,visibility",
              },
            );
          }
        });
      });
      return () => media.revert();
    },
    {
      dependencies: [mobileFiltersOpen, settingsOpen],
      scope: root,
      revertOnUpdate: true,
    },
  );

  useGSAP(
    () => {
      const panel = root.current;
      if (panel === null) {
        return;
      }
      const media = gsap.matchMedia();
      media.add(NO_MOTION, () => {
        const selectedControls = Array.from(
          panel.querySelectorAll(
            [
              ".tab[data-selected='true']",
              ".device-chip[data-selected='true']",
              ".mobile-dock button[data-selected='true'] .mobile-dock-icon",
            ].join(","),
          ),
        );
        gsap.fromTo(
          selectedControls,
          { scale: 0.9 },
          {
            scale: 1,
            duration: 0.36,
            ease: "back.out(1.8)",
            overwrite: "auto",
            clearProps: "transform",
          },
        );
      });
      return () => media.revert();
    },
    {
      dependencies: [deviceFilter, tab],
      scope: root,
      revertOnUpdate: true,
    },
  );

  useGSAP(
    () => {
      const panel = root.current;
      const dot = panel?.querySelector(".connection-dot");
      if (dot === null || dot === undefined || connectionTone !== "live") {
        return;
      }
      const media = gsap.matchMedia();
      media.add(NO_MOTION, () => {
        gsap.to(dot, {
          scale: 1.28,
          autoAlpha: 0.58,
          duration: 0.9,
          ease: "sine.inOut",
          repeat: -1,
          yoyo: true,
        });
      });
      return () => media.revert();
    },
    {
      dependencies: [connectionTone],
      scope: root,
      revertOnUpdate: true,
    },
  );
}
