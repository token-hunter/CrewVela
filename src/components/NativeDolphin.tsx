import { useId } from "react";
import dolphin from "../assets/native-agent-dolphin.png";
import dolphinBody from "../assets/native-agent-dolphin-body.png";
import "../styles/native-dolphin.css";

export function NativeDolphin() {
  const waterId = `dolphin-water-${useId().replace(/:/g, "")}`;
  return <div className="native-dolphin-swim">
    <div className="native-dolphin-travel">
      <div className="native-dolphin-heading">
        <div className="native-dolphin-pose">
          <div className="native-dolphin-gesture">
            <div className="native-dolphin-art">
              <img className="native-dolphin-layer native-dolphin-body" src={dolphinBody} alt="" />
              <img className="native-dolphin-layer native-dolphin-tail" src={dolphin} alt="" />
              <span className="native-dolphin-fin native-dolphin-fin--rear">
                <img className="native-dolphin-layer native-dolphin-fin-art" src={dolphin} alt="" />
              </span>
              <span className="native-dolphin-fin native-dolphin-fin--front">
                <img className="native-dolphin-layer native-dolphin-fin-art" src={dolphin} alt="" />
              </span>
              <svg className="native-dolphin-spray" viewBox="0 0 100 75" aria-hidden="true">
                <defs>
                  <linearGradient id={waterId} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0" stopColor="#51b8ed" stopOpacity=".65" />
                    <stop offset=".45" stopColor="#c8f3ff" stopOpacity=".9" />
                    <stop offset=".7" stopColor="#78d3f4" stopOpacity=".8" />
                    <stop offset="1" stopColor="#e9fbff" stopOpacity=".9" />
                  </linearGradient>
                </defs>
                <ellipse className="native-dolphin-blowhole" cx="78" cy="23.625" rx="1.2" ry="0.4125" />
                <g className="native-dolphin-jet" fill={`url(#${waterId})`}>
                  <path d="M77.4 23.4 C77.1 19.9 77.6 17.4 76.8 14.8 C76.1 12.7 76.8 11.2 77.8 11.8 C79.2 12.6 78.5 14.8 78.6 16.3 C78.7 18.8 78.7 21.1 78.8 23.4 Z" />
                  <path d="M77.5 23.4 C77.2 19.7 75.8 16.6 73.8 15.5 C72.4 14.8 71.8 13.5 72.6 12.9 C73.7 12.2 75.1 13.7 75.8 15.1 C77.4 17.8 78.2 20.6 78.2 23.4 Z" />
                  <path d="M78 23.4 C78.9 19.7 80 17.5 82.2 16.1 C83.6 15.2 84.1 13.8 83.2 13.4 C82 12.9 80.9 14.6 80.3 16 C79 18.4 78.1 20.9 77.7 23.4 Z" />
                  <path className="native-dolphin-water-highlight" d="M78 22.6 C77.8 19.2 78.3 16.3 77.5 13.2" />
                </g>
                <g className="native-dolphin-droplets" fill={`url(#${waterId})`}>
                  {[[-5, -9], [-2, -12], [2, -11], [5, -8], [-7, -5], [7, -4]].map(([x, y], index) => <g key={index} className={`native-dolphin-drop native-dolphin-drop--${index}`}>
                    <ellipse cx={78 + x} cy={23 + y} rx={index % 2 ? .65 : .85} ry={index % 2 ? .9 : 1.1} />
                    <ellipse className="native-dolphin-drop-highlight" cx={77.8 + x} cy={22.7 + y} rx=".2" ry=".3" />
                  </g>)}
                </g>
              </svg>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>;
}
