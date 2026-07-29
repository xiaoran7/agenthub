import { useRef } from "react";
import { useGSAP } from "@gsap/react";
import { gsap } from "gsap";

gsap.registerPlugin(useGSAP);

interface MotionMetricProps {
  value: number;
}

export function MotionMetric({ value }: MotionMetricProps) {
  const valueRef = useRef<HTMLSpanElement>(null);
  const previousValue = useRef(0);

  useGSAP(
    () => {
      const target = valueRef.current;
      if (target === null) {
        return;
      }
      const reduceMotion = window.matchMedia(
        "(prefers-reduced-motion: reduce)",
      ).matches;
      if (reduceMotion) {
        target.textContent = String(value);
        previousValue.current = value;
        return;
      }

      const counter = { value: previousValue.current };
      const tween = gsap.to(counter, {
        value,
        duration: 0.7,
        ease: "power2.out",
        overwrite: true,
        onUpdate: () => {
          target.textContent = String(Math.round(counter.value));
        },
        onComplete: () => {
          target.textContent = String(value);
          previousValue.current = value;
        },
      });

      return () => {
        tween.kill();
        previousValue.current = Number(target.textContent) || value;
      };
    },
    { dependencies: [value], revertOnUpdate: true },
  );

  return (
    <span className="motion-metric" ref={valueRef}>
      {value}
    </span>
  );
}
