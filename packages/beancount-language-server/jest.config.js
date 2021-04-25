module.exports = {
    preset: "ts-jest",
    testEnvironment: "node",
    testMatch: ["**/?(*.)+(spec|test).[tj]s?(x)"],
    collectCoverage: true,
    coverageReporters: ["lcov"],
    verbose: false,
    setupFilesAfterEnv: ["<rootDir>/tests/jest.setup.ts"]
};
