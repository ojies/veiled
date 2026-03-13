"use client";

interface Step {
  label: string;
  description?: string;
}

interface StepperProps {
  steps: Step[];
  activeStep: number;
}

export default function Stepper({ steps, activeStep }: StepperProps) {
  return (
    <div className="stepper">
      {steps.map((step, i) => {
        const state =
          i < activeStep
            ? "completed"
            : i === activeStep
            ? "active"
            : "locked";
        return (
          <div key={i} className={`stepper-step stepper-step--${state}`}>
            <div className="stepper-circle">
              {state === "completed" ? (
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                  <path
                    d="M2.5 7L5.5 10L11.5 4"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
              ) : (
                <span>{i + 1}</span>
              )}
            </div>
            <span className="stepper-label">{step.label}</span>
            {i < steps.length - 1 && <div className="stepper-line" />}
          </div>
        );
      })}
    </div>
  );
}
