import {useEffect, useState, type ReactNode} from 'react';
import {useLocation} from '@docusaurus/router';

import styles from './styles.module.css';

type Props = {
  children: ReactNode;
};

export default function Root({children}: Props): ReactNode {
  const location = useLocation();
  const [progress, setProgress] = useState(0);
  const [showTop, setShowTop] = useState(false);
  const isDoc = location.pathname.includes('/docs');

  useEffect(() => {
    if (!isDoc) {
      setProgress(0);
      setShowTop(false);
      return undefined;
    }

    function updateReadingPosition() {
      const scrollable = document.documentElement.scrollHeight - window.innerHeight;
      setProgress(scrollable > 0 ? Math.min(window.scrollY / scrollable, 1) : 0);
      setShowTop(window.scrollY > 640);
    }

    updateReadingPosition();
    window.addEventListener('scroll', updateReadingPosition, {passive: true});
    window.addEventListener('resize', updateReadingPosition);

    return () => {
      window.removeEventListener('scroll', updateReadingPosition);
      window.removeEventListener('resize', updateReadingPosition);
    };
  }, [isDoc, location.pathname]);

  function returnToTop() {
    window.scrollTo({top: 0, behavior: 'smooth'});
  }

  return (
    <>
      {isDoc && (
        <div className={styles.readingProgress} aria-hidden="true">
          <span style={{transform: `scaleX(${progress})`}} />
        </div>
      )}
      {children}
      {isDoc && (
        <button
          type="button"
          className={`${styles.backToTop} ${showTop ? styles.backToTopVisible : ''}`}
          onClick={returnToTop}
          aria-label="Back to the top">
          <span aria-hidden="true">↑</span>
          Top
        </button>
      )}
    </>
  );
}
