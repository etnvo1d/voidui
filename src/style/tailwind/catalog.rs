//! Shared utility definitions. Family order determines class conflict order.
//! Theme defaults are from Tailwind CSS v4.1.13 (MIT); see LICENSE-Tailwind.
//! https://github.com/tailwindlabs/tailwindcss/blob/v4.1.13/packages/tailwindcss/theme.css
//! Extend scales/families here to expose both CSS classes and Rust methods.
use super::{Utility, apply_utility};

voidui_macros::utility_catalog! {
    scale spacing {
        "0" => "0px";
        "0.5" => "calc(var(--spacing, 0.25rem) * 0.5)";
        "1" => "calc(var(--spacing, 0.25rem) * 1)";
        "1.5" => "calc(var(--spacing, 0.25rem) * 1.5)";
        "2" => "calc(var(--spacing, 0.25rem) * 2)";
        "2.5" => "calc(var(--spacing, 0.25rem) * 2.5)";
        "3" => "calc(var(--spacing, 0.25rem) * 3)";
        "3.5" => "calc(var(--spacing, 0.25rem) * 3.5)";
        "4" => "calc(var(--spacing, 0.25rem) * 4)";
        "5" => "calc(var(--spacing, 0.25rem) * 5)";
        "6" => "calc(var(--spacing, 0.25rem) * 6)";
        "7" => "calc(var(--spacing, 0.25rem) * 7)";
        "8" => "calc(var(--spacing, 0.25rem) * 8)";
        "9" => "calc(var(--spacing, 0.25rem) * 9)";
        "10" => "calc(var(--spacing, 0.25rem) * 10)";
        "11" => "calc(var(--spacing, 0.25rem) * 11)";
        "12" => "calc(var(--spacing, 0.25rem) * 12)";
        "14" => "calc(var(--spacing, 0.25rem) * 14)";
        "16" => "calc(var(--spacing, 0.25rem) * 16)";
        "20" => "calc(var(--spacing, 0.25rem) * 20)";
        "24" => "calc(var(--spacing, 0.25rem) * 24)";
        "28" => "calc(var(--spacing, 0.25rem) * 28)";
        "32" => "calc(var(--spacing, 0.25rem) * 32)";
        "36" => "calc(var(--spacing, 0.25rem) * 36)";
        "40" => "calc(var(--spacing, 0.25rem) * 40)";
        "44" => "calc(var(--spacing, 0.25rem) * 44)";
        "48" => "calc(var(--spacing, 0.25rem) * 48)";
        "52" => "calc(var(--spacing, 0.25rem) * 52)";
        "56" => "calc(var(--spacing, 0.25rem) * 56)";
        "60" => "calc(var(--spacing, 0.25rem) * 60)";
        "64" => "calc(var(--spacing, 0.25rem) * 64)";
        "72" => "calc(var(--spacing, 0.25rem) * 72)";
        "80" => "calc(var(--spacing, 0.25rem) * 80)";
        "96" => "calc(var(--spacing, 0.25rem) * 96)";
        "px" => "1px";
    }
    scale fractions {
        "1/2" => "50%";
        "1/3" => "calc(100% / 3)";
        "2/3" => "calc(200% / 3)";
        "1/4" => "25%";
        "2/4" => "50%";
        "3/4" => "75%";
        "1/5" => "20%";
        "2/5" => "40%";
        "3/5" => "60%";
        "4/5" => "80%";
        "1/6" => "calc(100% / 6)";
        "5/6" => "calc(500% / 6)";
    }
    scale colors {
        "red-50" => "var(--color-red-50, oklch(97.1% 0.013 17.38))";
        "red-100" => "var(--color-red-100, oklch(93.6% 0.032 17.717))";
        "red-200" => "var(--color-red-200, oklch(88.5% 0.062 18.334))";
        "red-300" => "var(--color-red-300, oklch(80.8% 0.114 19.571))";
        "red-400" => "var(--color-red-400, oklch(70.4% 0.191 22.216))";
        "red-500" => "var(--color-red-500, oklch(63.7% 0.237 25.331))";
        "red-600" => "var(--color-red-600, oklch(57.7% 0.245 27.325))";
        "red-700" => "var(--color-red-700, oklch(50.5% 0.213 27.518))";
        "red-800" => "var(--color-red-800, oklch(44.4% 0.177 26.899))";
        "red-900" => "var(--color-red-900, oklch(39.6% 0.141 25.723))";
        "red-950" => "var(--color-red-950, oklch(25.8% 0.092 26.042))";
        "orange-50" => "var(--color-orange-50, oklch(98% 0.016 73.684))";
        "orange-100" => "var(--color-orange-100, oklch(95.4% 0.038 75.164))";
        "orange-200" => "var(--color-orange-200, oklch(90.1% 0.076 70.697))";
        "orange-300" => "var(--color-orange-300, oklch(83.7% 0.128 66.29))";
        "orange-400" => "var(--color-orange-400, oklch(75% 0.183 55.934))";
        "orange-500" => "var(--color-orange-500, oklch(70.5% 0.213 47.604))";
        "orange-600" => "var(--color-orange-600, oklch(64.6% 0.222 41.116))";
        "orange-700" => "var(--color-orange-700, oklch(55.3% 0.195 38.402))";
        "orange-800" => "var(--color-orange-800, oklch(47% 0.157 37.304))";
        "orange-900" => "var(--color-orange-900, oklch(40.8% 0.123 38.172))";
        "orange-950" => "var(--color-orange-950, oklch(26.6% 0.079 36.259))";
        "amber-50" => "var(--color-amber-50, oklch(98.7% 0.022 95.277))";
        "amber-100" => "var(--color-amber-100, oklch(96.2% 0.059 95.617))";
        "amber-200" => "var(--color-amber-200, oklch(92.4% 0.12 95.746))";
        "amber-300" => "var(--color-amber-300, oklch(87.9% 0.169 91.605))";
        "amber-400" => "var(--color-amber-400, oklch(82.8% 0.189 84.429))";
        "amber-500" => "var(--color-amber-500, oklch(76.9% 0.188 70.08))";
        "amber-600" => "var(--color-amber-600, oklch(66.6% 0.179 58.318))";
        "amber-700" => "var(--color-amber-700, oklch(55.5% 0.163 48.998))";
        "amber-800" => "var(--color-amber-800, oklch(47.3% 0.137 46.201))";
        "amber-900" => "var(--color-amber-900, oklch(41.4% 0.112 45.904))";
        "amber-950" => "var(--color-amber-950, oklch(27.9% 0.077 45.635))";
        "yellow-50" => "var(--color-yellow-50, oklch(98.7% 0.026 102.212))";
        "yellow-100" => "var(--color-yellow-100, oklch(97.3% 0.071 103.193))";
        "yellow-200" => "var(--color-yellow-200, oklch(94.5% 0.129 101.54))";
        "yellow-300" => "var(--color-yellow-300, oklch(90.5% 0.182 98.111))";
        "yellow-400" => "var(--color-yellow-400, oklch(85.2% 0.199 91.936))";
        "yellow-500" => "var(--color-yellow-500, oklch(79.5% 0.184 86.047))";
        "yellow-600" => "var(--color-yellow-600, oklch(68.1% 0.162 75.834))";
        "yellow-700" => "var(--color-yellow-700, oklch(55.4% 0.135 66.442))";
        "yellow-800" => "var(--color-yellow-800, oklch(47.6% 0.114 61.907))";
        "yellow-900" => "var(--color-yellow-900, oklch(42.1% 0.095 57.708))";
        "yellow-950" => "var(--color-yellow-950, oklch(28.6% 0.066 53.813))";
        "lime-50" => "var(--color-lime-50, oklch(98.6% 0.031 120.757))";
        "lime-100" => "var(--color-lime-100, oklch(96.7% 0.067 122.328))";
        "lime-200" => "var(--color-lime-200, oklch(93.8% 0.127 124.321))";
        "lime-300" => "var(--color-lime-300, oklch(89.7% 0.196 126.665))";
        "lime-400" => "var(--color-lime-400, oklch(84.1% 0.238 128.85))";
        "lime-500" => "var(--color-lime-500, oklch(76.8% 0.233 130.85))";
        "lime-600" => "var(--color-lime-600, oklch(64.8% 0.2 131.684))";
        "lime-700" => "var(--color-lime-700, oklch(53.2% 0.157 131.589))";
        "lime-800" => "var(--color-lime-800, oklch(45.3% 0.124 130.933))";
        "lime-900" => "var(--color-lime-900, oklch(40.5% 0.101 131.063))";
        "lime-950" => "var(--color-lime-950, oklch(27.4% 0.072 132.109))";
        "green-50" => "var(--color-green-50, oklch(98.2% 0.018 155.826))";
        "green-100" => "var(--color-green-100, oklch(96.2% 0.044 156.743))";
        "green-200" => "var(--color-green-200, oklch(92.5% 0.084 155.995))";
        "green-300" => "var(--color-green-300, oklch(87.1% 0.15 154.449))";
        "green-400" => "var(--color-green-400, oklch(79.2% 0.209 151.711))";
        "green-500" => "var(--color-green-500, oklch(72.3% 0.219 149.579))";
        "green-600" => "var(--color-green-600, oklch(62.7% 0.194 149.214))";
        "green-700" => "var(--color-green-700, oklch(52.7% 0.154 150.069))";
        "green-800" => "var(--color-green-800, oklch(44.8% 0.119 151.328))";
        "green-900" => "var(--color-green-900, oklch(39.3% 0.095 152.535))";
        "green-950" => "var(--color-green-950, oklch(26.6% 0.065 152.934))";
        "emerald-50" => "var(--color-emerald-50, oklch(97.9% 0.021 166.113))";
        "emerald-100" => "var(--color-emerald-100, oklch(95% 0.052 163.051))";
        "emerald-200" => "var(--color-emerald-200, oklch(90.5% 0.093 164.15))";
        "emerald-300" => "var(--color-emerald-300, oklch(84.5% 0.143 164.978))";
        "emerald-400" => "var(--color-emerald-400, oklch(76.5% 0.177 163.223))";
        "emerald-500" => "var(--color-emerald-500, oklch(69.6% 0.17 162.48))";
        "emerald-600" => "var(--color-emerald-600, oklch(59.6% 0.145 163.225))";
        "emerald-700" => "var(--color-emerald-700, oklch(50.8% 0.118 165.612))";
        "emerald-800" => "var(--color-emerald-800, oklch(43.2% 0.095 166.913))";
        "emerald-900" => "var(--color-emerald-900, oklch(37.8% 0.077 168.94))";
        "emerald-950" => "var(--color-emerald-950, oklch(26.2% 0.051 172.552))";
        "teal-50" => "var(--color-teal-50, oklch(98.4% 0.014 180.72))";
        "teal-100" => "var(--color-teal-100, oklch(95.3% 0.051 180.801))";
        "teal-200" => "var(--color-teal-200, oklch(91% 0.096 180.426))";
        "teal-300" => "var(--color-teal-300, oklch(85.5% 0.138 181.071))";
        "teal-400" => "var(--color-teal-400, oklch(77.7% 0.152 181.912))";
        "teal-500" => "var(--color-teal-500, oklch(70.4% 0.14 182.503))";
        "teal-600" => "var(--color-teal-600, oklch(60% 0.118 184.704))";
        "teal-700" => "var(--color-teal-700, oklch(51.1% 0.096 186.391))";
        "teal-800" => "var(--color-teal-800, oklch(43.7% 0.078 188.216))";
        "teal-900" => "var(--color-teal-900, oklch(38.6% 0.063 188.416))";
        "teal-950" => "var(--color-teal-950, oklch(27.7% 0.046 192.524))";
        "cyan-50" => "var(--color-cyan-50, oklch(98.4% 0.019 200.873))";
        "cyan-100" => "var(--color-cyan-100, oklch(95.6% 0.045 203.388))";
        "cyan-200" => "var(--color-cyan-200, oklch(91.7% 0.08 205.041))";
        "cyan-300" => "var(--color-cyan-300, oklch(86.5% 0.127 207.078))";
        "cyan-400" => "var(--color-cyan-400, oklch(78.9% 0.154 211.53))";
        "cyan-500" => "var(--color-cyan-500, oklch(71.5% 0.143 215.221))";
        "cyan-600" => "var(--color-cyan-600, oklch(60.9% 0.126 221.723))";
        "cyan-700" => "var(--color-cyan-700, oklch(52% 0.105 223.128))";
        "cyan-800" => "var(--color-cyan-800, oklch(45% 0.085 224.283))";
        "cyan-900" => "var(--color-cyan-900, oklch(39.8% 0.07 227.392))";
        "cyan-950" => "var(--color-cyan-950, oklch(30.2% 0.056 229.695))";
        "sky-50" => "var(--color-sky-50, oklch(97.7% 0.013 236.62))";
        "sky-100" => "var(--color-sky-100, oklch(95.1% 0.026 236.824))";
        "sky-200" => "var(--color-sky-200, oklch(90.1% 0.058 230.902))";
        "sky-300" => "var(--color-sky-300, oklch(82.8% 0.111 230.318))";
        "sky-400" => "var(--color-sky-400, oklch(74.6% 0.16 232.661))";
        "sky-500" => "var(--color-sky-500, oklch(68.5% 0.169 237.323))";
        "sky-600" => "var(--color-sky-600, oklch(58.8% 0.158 241.966))";
        "sky-700" => "var(--color-sky-700, oklch(50% 0.134 242.749))";
        "sky-800" => "var(--color-sky-800, oklch(44.3% 0.11 240.79))";
        "sky-900" => "var(--color-sky-900, oklch(39.1% 0.09 240.876))";
        "sky-950" => "var(--color-sky-950, oklch(29.3% 0.066 243.157))";
        "blue-50" => "var(--color-blue-50, oklch(97% 0.014 254.604))";
        "blue-100" => "var(--color-blue-100, oklch(93.2% 0.032 255.585))";
        "blue-200" => "var(--color-blue-200, oklch(88.2% 0.059 254.128))";
        "blue-300" => "var(--color-blue-300, oklch(80.9% 0.105 251.813))";
        "blue-400" => "var(--color-blue-400, oklch(70.7% 0.165 254.624))";
        "blue-500" => "var(--color-blue-500, oklch(62.3% 0.214 259.815))";
        "blue-600" => "var(--color-blue-600, oklch(54.6% 0.245 262.881))";
        "blue-700" => "var(--color-blue-700, oklch(48.8% 0.243 264.376))";
        "blue-800" => "var(--color-blue-800, oklch(42.4% 0.199 265.638))";
        "blue-900" => "var(--color-blue-900, oklch(37.9% 0.146 265.522))";
        "blue-950" => "var(--color-blue-950, oklch(28.2% 0.091 267.935))";
        "indigo-50" => "var(--color-indigo-50, oklch(96.2% 0.018 272.314))";
        "indigo-100" => "var(--color-indigo-100, oklch(93% 0.034 272.788))";
        "indigo-200" => "var(--color-indigo-200, oklch(87% 0.065 274.039))";
        "indigo-300" => "var(--color-indigo-300, oklch(78.5% 0.115 274.713))";
        "indigo-400" => "var(--color-indigo-400, oklch(67.3% 0.182 276.935))";
        "indigo-500" => "var(--color-indigo-500, oklch(58.5% 0.233 277.117))";
        "indigo-600" => "var(--color-indigo-600, oklch(51.1% 0.262 276.966))";
        "indigo-700" => "var(--color-indigo-700, oklch(45.7% 0.24 277.023))";
        "indigo-800" => "var(--color-indigo-800, oklch(39.8% 0.195 277.366))";
        "indigo-900" => "var(--color-indigo-900, oklch(35.9% 0.144 278.697))";
        "indigo-950" => "var(--color-indigo-950, oklch(25.7% 0.09 281.288))";
        "violet-50" => "var(--color-violet-50, oklch(96.9% 0.016 293.756))";
        "violet-100" => "var(--color-violet-100, oklch(94.3% 0.029 294.588))";
        "violet-200" => "var(--color-violet-200, oklch(89.4% 0.057 293.283))";
        "violet-300" => "var(--color-violet-300, oklch(81.1% 0.111 293.571))";
        "violet-400" => "var(--color-violet-400, oklch(70.2% 0.183 293.541))";
        "violet-500" => "var(--color-violet-500, oklch(60.6% 0.25 292.717))";
        "violet-600" => "var(--color-violet-600, oklch(54.1% 0.281 293.009))";
        "violet-700" => "var(--color-violet-700, oklch(49.1% 0.27 292.581))";
        "violet-800" => "var(--color-violet-800, oklch(43.2% 0.232 292.759))";
        "violet-900" => "var(--color-violet-900, oklch(38% 0.189 293.745))";
        "violet-950" => "var(--color-violet-950, oklch(28.3% 0.141 291.089))";
        "purple-50" => "var(--color-purple-50, oklch(97.7% 0.014 308.299))";
        "purple-100" => "var(--color-purple-100, oklch(94.6% 0.033 307.174))";
        "purple-200" => "var(--color-purple-200, oklch(90.2% 0.063 306.703))";
        "purple-300" => "var(--color-purple-300, oklch(82.7% 0.119 306.383))";
        "purple-400" => "var(--color-purple-400, oklch(71.4% 0.203 305.504))";
        "purple-500" => "var(--color-purple-500, oklch(62.7% 0.265 303.9))";
        "purple-600" => "var(--color-purple-600, oklch(55.8% 0.288 302.321))";
        "purple-700" => "var(--color-purple-700, oklch(49.6% 0.265 301.924))";
        "purple-800" => "var(--color-purple-800, oklch(43.8% 0.218 303.724))";
        "purple-900" => "var(--color-purple-900, oklch(38.1% 0.176 304.987))";
        "purple-950" => "var(--color-purple-950, oklch(29.1% 0.149 302.717))";
        "fuchsia-50" => "var(--color-fuchsia-50, oklch(97.7% 0.017 320.058))";
        "fuchsia-100" => "var(--color-fuchsia-100, oklch(95.2% 0.037 318.852))";
        "fuchsia-200" => "var(--color-fuchsia-200, oklch(90.3% 0.076 319.62))";
        "fuchsia-300" => "var(--color-fuchsia-300, oklch(83.3% 0.145 321.434))";
        "fuchsia-400" => "var(--color-fuchsia-400, oklch(74% 0.238 322.16))";
        "fuchsia-500" => "var(--color-fuchsia-500, oklch(66.7% 0.295 322.15))";
        "fuchsia-600" => "var(--color-fuchsia-600, oklch(59.1% 0.293 322.896))";
        "fuchsia-700" => "var(--color-fuchsia-700, oklch(51.8% 0.253 323.949))";
        "fuchsia-800" => "var(--color-fuchsia-800, oklch(45.2% 0.211 324.591))";
        "fuchsia-900" => "var(--color-fuchsia-900, oklch(40.1% 0.17 325.612))";
        "fuchsia-950" => "var(--color-fuchsia-950, oklch(29.3% 0.136 325.661))";
        "pink-50" => "var(--color-pink-50, oklch(97.1% 0.014 343.198))";
        "pink-100" => "var(--color-pink-100, oklch(94.8% 0.028 342.258))";
        "pink-200" => "var(--color-pink-200, oklch(89.9% 0.061 343.231))";
        "pink-300" => "var(--color-pink-300, oklch(82.3% 0.12 346.018))";
        "pink-400" => "var(--color-pink-400, oklch(71.8% 0.202 349.761))";
        "pink-500" => "var(--color-pink-500, oklch(65.6% 0.241 354.308))";
        "pink-600" => "var(--color-pink-600, oklch(59.2% 0.249 0.584))";
        "pink-700" => "var(--color-pink-700, oklch(52.5% 0.223 3.958))";
        "pink-800" => "var(--color-pink-800, oklch(45.9% 0.187 3.815))";
        "pink-900" => "var(--color-pink-900, oklch(40.8% 0.153 2.432))";
        "pink-950" => "var(--color-pink-950, oklch(28.4% 0.109 3.907))";
        "rose-50" => "var(--color-rose-50, oklch(96.9% 0.015 12.422))";
        "rose-100" => "var(--color-rose-100, oklch(94.1% 0.03 12.58))";
        "rose-200" => "var(--color-rose-200, oklch(89.2% 0.058 10.001))";
        "rose-300" => "var(--color-rose-300, oklch(81% 0.117 11.638))";
        "rose-400" => "var(--color-rose-400, oklch(71.2% 0.194 13.428))";
        "rose-500" => "var(--color-rose-500, oklch(64.5% 0.246 16.439))";
        "rose-600" => "var(--color-rose-600, oklch(58.6% 0.253 17.585))";
        "rose-700" => "var(--color-rose-700, oklch(51.4% 0.222 16.935))";
        "rose-800" => "var(--color-rose-800, oklch(45.5% 0.188 13.697))";
        "rose-900" => "var(--color-rose-900, oklch(41% 0.159 10.272))";
        "rose-950" => "var(--color-rose-950, oklch(27.1% 0.105 12.094))";
        "slate-50" => "var(--color-slate-50, oklch(98.4% 0.003 247.858))";
        "slate-100" => "var(--color-slate-100, oklch(96.8% 0.007 247.896))";
        "slate-200" => "var(--color-slate-200, oklch(92.9% 0.013 255.508))";
        "slate-300" => "var(--color-slate-300, oklch(86.9% 0.022 252.894))";
        "slate-400" => "var(--color-slate-400, oklch(70.4% 0.04 256.788))";
        "slate-500" => "var(--color-slate-500, oklch(55.4% 0.046 257.417))";
        "slate-600" => "var(--color-slate-600, oklch(44.6% 0.043 257.281))";
        "slate-700" => "var(--color-slate-700, oklch(37.2% 0.044 257.287))";
        "slate-800" => "var(--color-slate-800, oklch(27.9% 0.041 260.031))";
        "slate-900" => "var(--color-slate-900, oklch(20.8% 0.042 265.755))";
        "slate-950" => "var(--color-slate-950, oklch(12.9% 0.042 264.695))";
        "gray-50" => "var(--color-gray-50, oklch(98.5% 0.002 247.839))";
        "gray-100" => "var(--color-gray-100, oklch(96.7% 0.003 264.542))";
        "gray-200" => "var(--color-gray-200, oklch(92.8% 0.006 264.531))";
        "gray-300" => "var(--color-gray-300, oklch(87.2% 0.01 258.338))";
        "gray-400" => "var(--color-gray-400, oklch(70.7% 0.022 261.325))";
        "gray-500" => "var(--color-gray-500, oklch(55.1% 0.027 264.364))";
        "gray-600" => "var(--color-gray-600, oklch(44.6% 0.03 256.802))";
        "gray-700" => "var(--color-gray-700, oklch(37.3% 0.034 259.733))";
        "gray-800" => "var(--color-gray-800, oklch(27.8% 0.033 256.848))";
        "gray-900" => "var(--color-gray-900, oklch(21% 0.034 264.665))";
        "gray-950" => "var(--color-gray-950, oklch(13% 0.028 261.692))";
        "zinc-50" => "var(--color-zinc-50, oklch(98.5% 0 0))";
        "zinc-100" => "var(--color-zinc-100, oklch(96.7% 0.001 286.375))";
        "zinc-200" => "var(--color-zinc-200, oklch(92% 0.004 286.32))";
        "zinc-300" => "var(--color-zinc-300, oklch(87.1% 0.006 286.286))";
        "zinc-400" => "var(--color-zinc-400, oklch(70.5% 0.015 286.067))";
        "zinc-500" => "var(--color-zinc-500, oklch(55.2% 0.016 285.938))";
        "zinc-600" => "var(--color-zinc-600, oklch(44.2% 0.017 285.786))";
        "zinc-700" => "var(--color-zinc-700, oklch(37% 0.013 285.805))";
        "zinc-800" => "var(--color-zinc-800, oklch(27.4% 0.006 286.033))";
        "zinc-900" => "var(--color-zinc-900, oklch(21% 0.006 285.885))";
        "zinc-950" => "var(--color-zinc-950, oklch(14.1% 0.005 285.823))";
        "neutral-50" => "var(--color-neutral-50, oklch(98.5% 0 0))";
        "neutral-100" => "var(--color-neutral-100, oklch(97% 0 0))";
        "neutral-200" => "var(--color-neutral-200, oklch(92.2% 0 0))";
        "neutral-300" => "var(--color-neutral-300, oklch(87% 0 0))";
        "neutral-400" => "var(--color-neutral-400, oklch(70.8% 0 0))";
        "neutral-500" => "var(--color-neutral-500, oklch(55.6% 0 0))";
        "neutral-600" => "var(--color-neutral-600, oklch(43.9% 0 0))";
        "neutral-700" => "var(--color-neutral-700, oklch(37.1% 0 0))";
        "neutral-800" => "var(--color-neutral-800, oklch(26.9% 0 0))";
        "neutral-900" => "var(--color-neutral-900, oklch(20.5% 0 0))";
        "neutral-950" => "var(--color-neutral-950, oklch(14.5% 0 0))";
        "stone-50" => "var(--color-stone-50, oklch(98.5% 0.001 106.423))";
        "stone-100" => "var(--color-stone-100, oklch(97% 0.001 106.424))";
        "stone-200" => "var(--color-stone-200, oklch(92.3% 0.003 48.717))";
        "stone-300" => "var(--color-stone-300, oklch(86.9% 0.005 56.366))";
        "stone-400" => "var(--color-stone-400, oklch(70.9% 0.01 56.259))";
        "stone-500" => "var(--color-stone-500, oklch(55.3% 0.013 58.071))";
        "stone-600" => "var(--color-stone-600, oklch(44.4% 0.011 73.639))";
        "stone-700" => "var(--color-stone-700, oklch(37.4% 0.01 67.558))";
        "stone-800" => "var(--color-stone-800, oklch(26.8% 0.007 34.298))";
        "stone-900" => "var(--color-stone-900, oklch(21.6% 0.006 56.043))";
        "stone-950" => "var(--color-stone-950, oklch(14.7% 0.004 49.25))";
        "black" => "var(--color-black, #000)";
        "white" => "var(--color-white, #fff)";
        "transparent" => "transparent";
        "current" => "currentColor";
        "inherit" => "inherit";
    }
    scale border_widths {
        "0" => "0px";
        "2" => "2px";
        "4" => "4px";
        "8" => "8px";
    }
    scale radii {
        "xs" => "var(--radius-xs, 0.125rem)";
        "sm" => "var(--radius-sm, 0.25rem)";
        "md" => "var(--radius-md, 0.375rem)";
        "lg" => "var(--radius-lg, 0.5rem)";
        "xl" => "var(--radius-xl, 0.75rem)";
        "2xl" => "var(--radius-2xl, 1rem)";
        "3xl" => "var(--radius-3xl, 1.5rem)";
        "4xl" => "var(--radius-4xl, 2rem)";
        "none" => "0px";
        "full" => "9999px";
    }
    scale weights {
        "thin" => "var(--font-weight-thin, 100)";
        "extralight" => "var(--font-weight-extralight, 200)";
        "light" => "var(--font-weight-light, 300)";
        "normal" => "var(--font-weight-normal, 400)";
        "medium" => "var(--font-weight-medium, 500)";
        "semibold" => "var(--font-weight-semibold, 600)";
        "bold" => "var(--font-weight-bold, 700)";
        "extrabold" => "var(--font-weight-extrabold, 800)";
        "black" => "var(--font-weight-black, 900)";
    }
    scale leading {
        "tight" => "var(--leading-tight, 1.25)";
        "snug" => "var(--leading-snug, 1.375)";
        "normal" => "var(--leading-normal, 1.5)";
        "relaxed" => "var(--leading-relaxed, 1.625)";
        "loose" => "var(--leading-loose, 2)";
        "none" => "1";
    }
    scale shadows {
        "2xs" => "var(--shadow-2xs, 0 1px rgb(0 0 0 / 0.05))";
        "xs" => "var(--shadow-xs, 0 1px 2px 0 rgb(0 0 0 / 0.05))";
        "sm" => "var(--shadow-sm, 0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1))";
        "md" => "var(--shadow-md, 0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1))";
        "lg" => "var(--shadow-lg, 0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1))";
        "xl" => "var(--shadow-xl, 0 20px 25px -5px rgb(0 0 0 / 0.1), 0 8px 10px -6px rgb(0 0 0 / 0.1))";
        "2xl" => "var(--shadow-2xl, 0 25px 50px -12px rgb(0 0 0 / 0.25))";
        "inner" => "var(--shadow-inner, inset 0 2px 4px 0 rgb(0 0 0 / 0.05))";
        "none" => "none";
    }
    existing block "block" { "display" => "block"; }
    existing flex "flex" { "display" => "flex"; }
    existing grid "grid" { "display" => "grid"; }
    existing hidden "hidden" { "display" => "none"; }
    existing relative "relative" { "position" => "relative"; }
    existing absolute "absolute" { "position" => "absolute"; }
    existing fixed "fixed" { "position" => "fixed"; }
    existing sticky "sticky" { "position" => "sticky"; }
    existing static_position "static" { "position" => "static"; }
    existing flex_row "flex-row" { "flex-direction" => "row"; }
    existing flex_col "flex-col" { "flex-direction" => "column"; }
    existing flex_row_reverse "flex-row-reverse" { "flex-direction" => "row-reverse"; }
    existing flex_col_reverse "flex-col-reverse" { "flex-direction" => "column-reverse"; }
    utility flex_wrap_on "flex-wrap" { "flex-wrap" => "wrap"; }
    utility flex_nowrap "flex-nowrap" { "flex-wrap" => "nowrap"; }
    utility flex_wrap_reverse "flex-wrap-reverse" { "flex-wrap" => "wrap-reverse"; }
    utility flex_1 "flex-1" { "flex" => "1"; }
    utility flex_auto "flex-auto" { "flex" => "auto"; }
    utility flex_initial "flex-initial" { "flex" => "initial"; }
    utility flex_none "flex-none" { "flex" => "none"; }
    utility grow "grow" { "flex-grow" => "1"; }
    utility grow_0 "grow-0" { "flex-grow" => "0"; }
    utility shrink "shrink" { "flex-shrink" => "1"; }
    utility shrink_0 "shrink-0" { "flex-shrink" => "0"; }
    utility items_start "items-start" { "align-items" => "flex-start"; }
    utility items_end "items-end" { "align-items" => "flex-end"; }
    existing items_center "items-center" { "align-items" => "center"; }
    utility items_baseline "items-baseline" { "align-items" => "baseline"; }
    utility items_stretch "items-stretch" { "align-items" => "stretch"; }
    utility justify_start "justify-start" { "justify-content" => "flex-start"; }
    utility justify_end "justify-end" { "justify-content" => "flex-end"; }
    existing justify_center "justify-center" { "justify-content" => "center"; }
    existing justify_between "justify-between" { "justify-content" => "space-between"; }
    utility justify_around "justify-around" { "justify-content" => "space-around"; }
    utility justify_evenly "justify-evenly" { "justify-content" => "space-evenly"; }
    utility self_auto "self-auto" { "align-self" => "auto"; }
    utility self_start "self-start" { "align-self" => "flex-start"; }
    utility self_end "self-end" { "align-self" => "flex-end"; }
    utility self_center "self-center" { "align-self" => "center"; }
    utility self_stretch "self-stretch" { "align-self" => "stretch"; }
    // Explicit zero lengths keep grid tracks compatible with the Taffy parser.
    utility grid_cols_1 "grid-cols-1" { "grid-template-columns" => "repeat(1, minmax(0px, 1fr))"; }
    utility grid_rows_1 "grid-rows-1" { "grid-template-rows" => "repeat(1, minmax(0px, 1fr))"; }
    utility col_span_1 "col-span-1" { "grid-column" => "span 1 / span 1"; }
    utility row_span_1 "row-span-1" { "grid-row" => "span 1 / span 1"; }
    utility grid_cols_2 "grid-cols-2" { "grid-template-columns" => "repeat(2, minmax(0px, 1fr))"; }
    utility grid_rows_2 "grid-rows-2" { "grid-template-rows" => "repeat(2, minmax(0px, 1fr))"; }
    utility col_span_2 "col-span-2" { "grid-column" => "span 2 / span 2"; }
    utility row_span_2 "row-span-2" { "grid-row" => "span 2 / span 2"; }
    utility grid_cols_3 "grid-cols-3" { "grid-template-columns" => "repeat(3, minmax(0px, 1fr))"; }
    utility grid_rows_3 "grid-rows-3" { "grid-template-rows" => "repeat(3, minmax(0px, 1fr))"; }
    utility col_span_3 "col-span-3" { "grid-column" => "span 3 / span 3"; }
    utility row_span_3 "row-span-3" { "grid-row" => "span 3 / span 3"; }
    utility grid_cols_4 "grid-cols-4" { "grid-template-columns" => "repeat(4, minmax(0px, 1fr))"; }
    utility grid_rows_4 "grid-rows-4" { "grid-template-rows" => "repeat(4, minmax(0px, 1fr))"; }
    utility col_span_4 "col-span-4" { "grid-column" => "span 4 / span 4"; }
    utility row_span_4 "row-span-4" { "grid-row" => "span 4 / span 4"; }
    utility grid_cols_5 "grid-cols-5" { "grid-template-columns" => "repeat(5, minmax(0px, 1fr))"; }
    utility grid_rows_5 "grid-rows-5" { "grid-template-rows" => "repeat(5, minmax(0px, 1fr))"; }
    utility col_span_5 "col-span-5" { "grid-column" => "span 5 / span 5"; }
    utility row_span_5 "row-span-5" { "grid-row" => "span 5 / span 5"; }
    utility grid_cols_6 "grid-cols-6" { "grid-template-columns" => "repeat(6, minmax(0px, 1fr))"; }
    utility grid_rows_6 "grid-rows-6" { "grid-template-rows" => "repeat(6, minmax(0px, 1fr))"; }
    utility col_span_6 "col-span-6" { "grid-column" => "span 6 / span 6"; }
    utility row_span_6 "row-span-6" { "grid-row" => "span 6 / span 6"; }
    utility grid_cols_7 "grid-cols-7" { "grid-template-columns" => "repeat(7, minmax(0px, 1fr))"; }
    utility grid_rows_7 "grid-rows-7" { "grid-template-rows" => "repeat(7, minmax(0px, 1fr))"; }
    utility col_span_7 "col-span-7" { "grid-column" => "span 7 / span 7"; }
    utility row_span_7 "row-span-7" { "grid-row" => "span 7 / span 7"; }
    utility grid_cols_8 "grid-cols-8" { "grid-template-columns" => "repeat(8, minmax(0px, 1fr))"; }
    utility grid_rows_8 "grid-rows-8" { "grid-template-rows" => "repeat(8, minmax(0px, 1fr))"; }
    utility col_span_8 "col-span-8" { "grid-column" => "span 8 / span 8"; }
    utility row_span_8 "row-span-8" { "grid-row" => "span 8 / span 8"; }
    utility grid_cols_9 "grid-cols-9" { "grid-template-columns" => "repeat(9, minmax(0px, 1fr))"; }
    utility grid_rows_9 "grid-rows-9" { "grid-template-rows" => "repeat(9, minmax(0px, 1fr))"; }
    utility col_span_9 "col-span-9" { "grid-column" => "span 9 / span 9"; }
    utility row_span_9 "row-span-9" { "grid-row" => "span 9 / span 9"; }
    utility grid_cols_10 "grid-cols-10" { "grid-template-columns" => "repeat(10, minmax(0px, 1fr))"; }
    utility grid_rows_10 "grid-rows-10" { "grid-template-rows" => "repeat(10, minmax(0px, 1fr))"; }
    utility col_span_10 "col-span-10" { "grid-column" => "span 10 / span 10"; }
    utility row_span_10 "row-span-10" { "grid-row" => "span 10 / span 10"; }
    utility grid_cols_11 "grid-cols-11" { "grid-template-columns" => "repeat(11, minmax(0px, 1fr))"; }
    utility grid_rows_11 "grid-rows-11" { "grid-template-rows" => "repeat(11, minmax(0px, 1fr))"; }
    utility col_span_11 "col-span-11" { "grid-column" => "span 11 / span 11"; }
    utility row_span_11 "row-span-11" { "grid-row" => "span 11 / span 11"; }
    utility grid_cols_12 "grid-cols-12" { "grid-template-columns" => "repeat(12, minmax(0px, 1fr))"; }
    utility grid_rows_12 "grid-rows-12" { "grid-template-rows" => "repeat(12, minmax(0px, 1fr))"; }
    utility col_span_12 "col-span-12" { "grid-column" => "span 12 / span 12"; }
    utility row_span_12 "row-span-12" { "grid-row" => "span 12 / span 12"; }
    family "w" spacing ["width"];
    family "w" fractions ["width"];
    utility w_full "w-full" { "width" => "100%"; }
    utility w_auto "w-auto" { "width" => "auto"; }
    family "h" spacing ["height"];
    family "h" fractions ["height"];
    utility h_full "h-full" { "height" => "100%"; }
    utility h_auto "h-auto" { "height" => "auto"; }
    family "min-w" spacing ["min-width"];
    family "min-w" fractions ["min-width"];
    utility min_w_full "min-w-full" { "min-width" => "100%"; }
    utility min_w_auto "min-w-auto" { "min-width" => "auto"; }
    family "min-h" spacing ["min-height"];
    family "min-h" fractions ["min-height"];
    utility min_h_full "min-h-full" { "min-height" => "100%"; }
    utility min_h_auto "min-h-auto" { "min-height" => "auto"; }
    family "max-w" spacing ["max-width"];
    family "max-w" fractions ["max-width"];
    utility max_w_full "max-w-full" { "max-width" => "100%"; }
    utility max_w_none "max-w-none" { "max-width" => "none"; }
    family "max-h" spacing ["max-height"];
    family "max-h" fractions ["max-height"];
    utility max_h_full "max-h-full" { "max-height" => "100%"; }
    utility max_h_none "max-h-none" { "max-height" => "none"; }
    family "basis" spacing ["flex-basis"];
    family "basis" fractions ["flex-basis"];
    utility basis_full "basis-full" { "flex-basis" => "100%"; }
    utility basis_auto "basis-auto" { "flex-basis" => "auto"; }
    utility w_screen "w-screen" { "width" => "100vw"; }
    utility w_min "w-min" { "width" => "min-content"; }
    utility w_max "w-max" { "width" => "max-content"; }
    utility w_fit "w-fit" { "width" => "fit-content"; }
    utility h_screen "h-screen" { "height" => "100vh"; }
    utility h_min "h-min" { "height" => "min-content"; }
    utility h_max "h-max" { "height" => "max-content"; }
    utility h_fit "h-fit" { "height" => "fit-content"; }
    family "size" spacing ["width", "height"];
    family "size" fractions ["width", "height"];
    utility size_full "size-full" { "width" => "100%"; "height" => "100%"; }
    utility size_auto "size-auto" { "width" => "auto"; "height" => "auto"; }
    family "gap" spacing ["gap"];
    family "gap-x" spacing ["column-gap"];
    family "gap-y" spacing ["row-gap"];
    family "p" spacing ["padding"];
    family "px" spacing ["padding-left", "padding-right"];
    family "py" spacing ["padding-top", "padding-bottom"];
    family "pt" spacing ["padding-top"];
    family "pr" spacing ["padding-right"];
    family "pb" spacing ["padding-bottom"];
    family "pl" spacing ["padding-left"];
    family "m" spacing ["margin"];
    utility m_auto "m-auto" { "margin" => "auto"; }
    family "mx" spacing ["margin-left", "margin-right"];
    utility mx_auto "mx-auto" { "margin-left" => "auto"; "margin-right" => "auto"; }
    family "my" spacing ["margin-top", "margin-bottom"];
    utility my_auto "my-auto" { "margin-top" => "auto"; "margin-bottom" => "auto"; }
    family "mt" spacing ["margin-top"];
    utility mt_auto "mt-auto" { "margin-top" => "auto"; }
    family "mr" spacing ["margin-right"];
    utility mr_auto "mr-auto" { "margin-right" => "auto"; }
    family "mb" spacing ["margin-bottom"];
    utility mb_auto "mb-auto" { "margin-bottom" => "auto"; }
    family "ml" spacing ["margin-left"];
    utility ml_auto "ml-auto" { "margin-left" => "auto"; }
    family "inset" spacing ["inset"];
    family "inset-x" spacing ["left", "right"];
    family "inset-y" spacing ["top", "bottom"];
    family "top" spacing ["top"];
    family "right" spacing ["right"];
    family "bottom" spacing ["bottom"];
    family "left" spacing ["left"];
    utility border_1 "border" { "border-width" => "1px"; }
    family "border" border_widths ["border-width"];
    utility border_x_1 "border-x" { "border-left-width" => "1px"; "border-right-width" => "1px"; }
    family "border-x" border_widths ["border-left-width", "border-right-width"];
    utility border_y_1 "border-y" { "border-top-width" => "1px"; "border-bottom-width" => "1px"; }
    family "border-y" border_widths ["border-top-width", "border-bottom-width"];
    utility border_t_1 "border-t" { "border-top-width" => "1px"; }
    family "border-t" border_widths ["border-top-width"];
    utility border_r_1 "border-r" { "border-right-width" => "1px"; }
    family "border-r" border_widths ["border-right-width"];
    utility border_b_1 "border-b" { "border-bottom-width" => "1px"; }
    family "border-b" border_widths ["border-bottom-width"];
    utility border_l_1 "border-l" { "border-left-width" => "1px"; }
    family "border-l" border_widths ["border-left-width"];
    family "rounded" radii ["border-radius"];
    utility text_xs "text-xs" { "font-size" => "var(--text-xs, 0.75rem)"; "line-height" => "var(--text-xs--line-height, calc(1 / 0.75))"; }
    utility text_sm "text-sm" { "font-size" => "var(--text-sm, 0.875rem)"; "line-height" => "var(--text-sm--line-height, calc(1.25 / 0.875))"; }
    utility text_base "text-base" { "font-size" => "var(--text-base, 1rem)"; "line-height" => "var(--text-base--line-height, calc(1.5 / 1))"; }
    utility text_lg "text-lg" { "font-size" => "var(--text-lg, 1.125rem)"; "line-height" => "var(--text-lg--line-height, calc(1.75 / 1.125))"; }
    utility text_xl "text-xl" { "font-size" => "var(--text-xl, 1.25rem)"; "line-height" => "var(--text-xl--line-height, calc(1.75 / 1.25))"; }
    utility text_2xl "text-2xl" { "font-size" => "var(--text-2xl, 1.5rem)"; "line-height" => "var(--text-2xl--line-height, calc(2 / 1.5))"; }
    utility text_3xl "text-3xl" { "font-size" => "var(--text-3xl, 1.875rem)"; "line-height" => "var(--text-3xl--line-height, calc(2.25 / 1.875))"; }
    utility text_4xl "text-4xl" { "font-size" => "var(--text-4xl, 2.25rem)"; "line-height" => "var(--text-4xl--line-height, calc(2.5 / 2.25))"; }
    utility text_5xl "text-5xl" { "font-size" => "var(--text-5xl, 3rem)"; "line-height" => "var(--text-5xl--line-height, 1)"; }
    utility text_6xl "text-6xl" { "font-size" => "var(--text-6xl, 3.75rem)"; "line-height" => "var(--text-6xl--line-height, 1)"; }
    utility text_7xl "text-7xl" { "font-size" => "var(--text-7xl, 4.5rem)"; "line-height" => "var(--text-7xl--line-height, 1)"; }
    utility text_8xl "text-8xl" { "font-size" => "var(--text-8xl, 6rem)"; "line-height" => "var(--text-8xl--line-height, 1)"; }
    utility text_9xl "text-9xl" { "font-size" => "var(--text-9xl, 8rem)"; "line-height" => "var(--text-9xl--line-height, 1)"; }
    family "font" weights ["font-weight"];
    family "leading" leading ["line-height"];
    utility text_left "text-left" { "text-align" => "left"; }
    utility text_center "text-center" { "text-align" => "center"; }
    utility text_right "text-right" { "text-align" => "right"; }
    utility text_start "text-start" { "text-align" => "start"; }
    utility text_end "text-end" { "text-align" => "end"; }
    existing italic "italic" { "font-style" => "italic"; }
    utility not_italic "not-italic" { "font-style" => "normal"; }
    utility text_wrap "text-wrap" { "text-wrap" => "wrap"; }
    utility text_nowrap "text-nowrap" { "text-wrap" => "nowrap"; }
    family "bg" colors ["background-color"];
    family "text" colors ["color"];
    family "border" colors ["border-color"];
    family "shadow" shadows ["box-shadow"];
    utility overflow_auto "overflow-auto" { "overflow" => "auto"; }
    utility overflow_hidden "overflow-hidden" { "overflow" => "hidden"; }
    utility overflow_clip "overflow-clip" { "overflow" => "clip"; }
    utility overflow_visible "overflow-visible" { "overflow" => "visible"; }
    utility overflow_scroll "overflow-scroll" { "overflow" => "scroll"; }
    utility overflow_x_auto "overflow-x-auto" { "overflow-x" => "auto"; }
    utility overflow_x_hidden "overflow-x-hidden" { "overflow-x" => "hidden"; }
    utility overflow_x_clip "overflow-x-clip" { "overflow-x" => "clip"; }
    utility overflow_x_visible "overflow-x-visible" { "overflow-x" => "visible"; }
    utility overflow_x_scroll "overflow-x-scroll" { "overflow-x" => "scroll"; }
    utility overflow_y_auto "overflow-y-auto" { "overflow-y" => "auto"; }
    utility overflow_y_hidden "overflow-y-hidden" { "overflow-y" => "hidden"; }
    utility overflow_y_clip "overflow-y-clip" { "overflow-y" => "clip"; }
    utility overflow_y_visible "overflow-y-visible" { "overflow-y" => "visible"; }
    utility overflow_y_scroll "overflow-y-scroll" { "overflow-y" => "scroll"; }
    // Complete standard keyword set: keep native cursors available in both APIs.
    utility cursor_auto "cursor-auto" { "cursor" => "auto"; }
    utility cursor_default "cursor-default" { "cursor" => "default"; }
    utility cursor_pointer "cursor-pointer" { "cursor" => "pointer"; }
    utility cursor_text "cursor-text" { "cursor" => "text"; }
    utility cursor_move "cursor-move" { "cursor" => "move"; }
    utility cursor_not_allowed "cursor-not-allowed" { "cursor" => "not-allowed"; }
    utility cursor_grab "cursor-grab" { "cursor" => "grab"; }
    utility cursor_grabbing "cursor-grabbing" { "cursor" => "grabbing"; }
    utility cursor_wait "cursor-wait" { "cursor" => "wait"; }
    utility cursor_help "cursor-help" { "cursor" => "help"; }
    utility cursor_none "cursor-none" { "cursor" => "none"; }
    utility cursor_context_menu "cursor-context-menu" { "cursor" => "context-menu"; }
    utility cursor_progress "cursor-progress" { "cursor" => "progress"; }
    utility cursor_cell "cursor-cell" { "cursor" => "cell"; }
    utility cursor_crosshair "cursor-crosshair" { "cursor" => "crosshair"; }
    utility cursor_vertical_text "cursor-vertical-text" { "cursor" => "vertical-text"; }
    utility cursor_alias "cursor-alias" { "cursor" => "alias"; }
    utility cursor_copy "cursor-copy" { "cursor" => "copy"; }
    utility cursor_no_drop "cursor-no-drop" { "cursor" => "no-drop"; }
    utility cursor_all_scroll "cursor-all-scroll" { "cursor" => "all-scroll"; }
    utility cursor_col_resize "cursor-col-resize" { "cursor" => "col-resize"; }
    utility cursor_row_resize "cursor-row-resize" { "cursor" => "row-resize"; }
    utility cursor_n_resize "cursor-n-resize" { "cursor" => "n-resize"; }
    utility cursor_e_resize "cursor-e-resize" { "cursor" => "e-resize"; }
    utility cursor_s_resize "cursor-s-resize" { "cursor" => "s-resize"; }
    utility cursor_w_resize "cursor-w-resize" { "cursor" => "w-resize"; }
    utility cursor_ne_resize "cursor-ne-resize" { "cursor" => "ne-resize"; }
    utility cursor_nw_resize "cursor-nw-resize" { "cursor" => "nw-resize"; }
    utility cursor_se_resize "cursor-se-resize" { "cursor" => "se-resize"; }
    utility cursor_sw_resize "cursor-sw-resize" { "cursor" => "sw-resize"; }
    utility cursor_ew_resize "cursor-ew-resize" { "cursor" => "ew-resize"; }
    utility cursor_ns_resize "cursor-ns-resize" { "cursor" => "ns-resize"; }
    utility cursor_nesw_resize "cursor-nesw-resize" { "cursor" => "nesw-resize"; }
    utility cursor_nwse_resize "cursor-nwse-resize" { "cursor" => "nwse-resize"; }
    utility cursor_zoom_in "cursor-zoom-in" { "cursor" => "zoom-in"; }
    utility cursor_zoom_out "cursor-zoom-out" { "cursor" => "zoom-out"; }
    utility select_none "select-none" { "user-select" => "none"; }
    utility select_text "select-text" { "user-select" => "text"; }
    utility select_all "select-all" { "user-select" => "all"; }
    utility select_auto "select-auto" { "user-select" => "auto"; }
    utility pointer_events_none "pointer-events-none" { "pointer-events" => "none"; }
    utility pointer_events_auto "pointer-events-auto" { "pointer-events" => "auto"; }
    utility visible "visible" { "visibility" => "visible"; }
    utility invisible "invisible" { "visibility" => "hidden"; }
    utility box_border "box-border" { "box-sizing" => "border-box"; }
    utility box_content "box-content" { "box-sizing" => "content-box"; }
    utility opacity_0 "opacity-0" { "opacity" => "0.0"; }
    utility opacity_5 "opacity-5" { "opacity" => "0.05"; }
    utility opacity_10 "opacity-10" { "opacity" => "0.1"; }
    utility opacity_20 "opacity-20" { "opacity" => "0.2"; }
    utility opacity_25 "opacity-25" { "opacity" => "0.25"; }
    utility opacity_30 "opacity-30" { "opacity" => "0.3"; }
    utility opacity_40 "opacity-40" { "opacity" => "0.4"; }
    utility opacity_50 "opacity-50" { "opacity" => "0.5"; }
    utility opacity_60 "opacity-60" { "opacity" => "0.6"; }
    utility opacity_70 "opacity-70" { "opacity" => "0.7"; }
    utility opacity_75 "opacity-75" { "opacity" => "0.75"; }
    utility opacity_80 "opacity-80" { "opacity" => "0.8"; }
    utility opacity_90 "opacity-90" { "opacity" => "0.9"; }
    utility opacity_95 "opacity-95" { "opacity" => "0.95"; }
    utility opacity_100 "opacity-100" { "opacity" => "1.0"; }
    utility z_0 "z-0" { "z-index" => "0"; }
    utility z_10 "z-10" { "z-index" => "10"; }
    utility z_20 "z-20" { "z-index" => "20"; }
    utility z_30 "z-30" { "z-index" => "30"; }
    utility z_40 "z-40" { "z-index" => "40"; }
    utility z_50 "z-50" { "z-index" => "50"; }
    utility z_auto "z-auto" { "z-index" => "auto"; }
}

#[doc(hidden)]
pub use __voidui_tailwind_styles;
